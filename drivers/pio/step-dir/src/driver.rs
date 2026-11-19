use embassy_rp::{
    clocks::clk_sys_freq,
    dma,
    gpio::Level,
    pio::{
        Common, Config as SmConfig, Direction as PinDirection, Instance, Irq, IrqFlags,
        LoadedProgram, Pin, ShiftDirection, StateMachine,
    },
};
use fixed::types::U24F8;
use pio::{InstructionOperands, MovDestination, MovOperation, MovSource};

use crate::{
    Config, Direction, Error, Progress, Segment, StopReason, StopReport, program, queue::Queue,
};

/// Caller-owned pins. No adjacency requirement; STEP is active high.
pub struct Pins<'d, P: Instance> {
    /// Rising edge is counted as one pulse.
    pub step: Pin<'d, P>,
    /// Low is Negative; high is Positive.
    pub dir: Pin<'d, P>,
}

/// One axis: one SM, 28 instructions, one DMA channel, four outstanding commands.
///
/// Commands are supplied through DMA, with no allocator, executor, or timer
/// dependency. Only accepted commands are committed; keep replaceable plans in
/// the caller. Poll/refill before exhausting the bounded queue. No per-pulse CPU
/// interrupts are needed. Completed-segment RX records are drained by `poll`.
///
/// This type owns its SM, IRQ flag and pins exclusively. Do not clear its IRQ
/// from another task or change clk_sys while it exists. Constructor leaves STEP
/// low and the engine disabled. No driver-enable or brake GPIO is owned here.
pub struct PioStepDir<'d, P: Instance, const SM: usize> {
    sm: StateMachine<'d, P, SM>,
    dma: dma::Channel<'d>,
    irq: Irq<'d, P, SM>,
    flags: IrqFlags<'d, P>,
    pins: Pins<'d, P>,
    program: LoadedProgram<'d, P>,
    sm_config: SmConfig<'d, P>,
    timing: Config,
    queue: Queue,
    running: bool,
    report: Option<StopReport>,
}

impl<'d, P: Instance, const SM: usize> PioStepDir<'d, P, SM> {
    /// Configure an idle engine. Bind Embassy's PIO and DMA interrupt handlers.
    /// Invalid resources/configuration are consumed on error. No motion starts.
    pub fn new(
        common: &mut Common<'d, P>,
        mut sm: StateMachine<'d, P, SM>,
        dma: dma::Channel<'d>,
        irq: Irq<'d, P, SM>,
        flags: IrqFlags<'d, P>,
        mut pins: Pins<'d, P>,
        timing: Config,
    ) -> Result<Self, Error> {
        let divider = timing.divider(clk_sys_freq())?;
        if pins.step.pin() == pins.dir.pin() || pins.step.pin() > 29 || pins.dir.pin() > 29 {
            return Err(Error::Pins);
        }
        sm.set_enable(false);
        sm.clear_fifos();
        sm.restart();
        let program = common
            .try_load_program(&program::program(timing))
            .map_err(|_| Error::ProgramMemory)?;
        let mut sm_config = SmConfig::default();
        sm_config.use_program(&program, &[&pins.step]);
        sm_config.set_out_pins(&[&pins.dir]);
        sm_config.shift_out.direction = ShiftDirection::Right;
        sm_config.shift_out.auto_fill = false;
        sm_config.shift_in.auto_fill = false;
        sm_config.clock_divider = U24F8::from_num(divider);
        sm.set_config(&sm_config);
        sm.set_pins(Level::Low, &[&pins.step, &pins.dir]);
        pins.step.set_output_inversion(false);
        pins.dir.set_output_inversion(false);
        pins.step.set_output_enable_inversion(false);
        pins.dir.set_output_enable_inversion(false);
        sm.set_pin_dirs(PinDirection::Out, &[&pins.step, &pins.dir]);
        let mut driver = Self {
            sm,
            dma,
            irq,
            flags,
            pins,
            program,
            sm_config,
            timing,
            queue: Queue::new(Direction::Negative),
            running: false,
            report: None,
        };
        driver.initialize();
        Ok(driver)
    }

    /// Atomically validate/admit up to four commands, then DMA their words to PIO.
    /// Returns after delivery, NOT after physical execution. Empty batches are no-ops.
    /// End is accepted only as the final command; it closes further admission.
    ///
    /// `QueueFull`/`Horizon` reject the entire batch without changing motion. Poll
    /// and retry later, or replace the caller's still-uncommitted plan. Accepted
    /// commands cannot be replaced; abort is the only way to discard them.
    ///
    /// Dropping this future after admission aborts DMA synchronously, stops PIO,
    /// clears pending work and stores a Cancelled report. Dropping an unpolled
    /// future has no effect. Starvation during DMA returns Stopped; inspect poll.
    pub async fn commit(&mut self, segments: &[Segment]) -> Result<(), Error> {
        if self.poll().is_err() {
            return Err(Error::Stopped);
        }
        let (next, words) = self.queue.prepare(self.timing, segments)?;
        if segments.is_empty() {
            return Ok(());
        }
        self.queue = next;
        let mut guard = CancelGuard {
            driver: self,
            armed: true,
        };
        guard
            .driver
            .sm
            .tx()
            .dma_push(&mut guard.driver.dma, &words[..segments.len()], false)
            .await;
        guard.armed = false;
        match guard.driver.poll() {
            Ok(_) => Ok(()),
            // End may physically finish before the DMA future is serviced.
            Err(report) if report.reason == StopReason::Completed => Ok(()),
            Err(_) => Err(Error::Stopped),
        }
    }

    /// Start preloaded work. The initial 17-tick low guard honors DIR hold even
    /// after an abort. No data is pulled before this call.
    pub fn start(&mut self) -> Result<(), Error> {
        if self.report.is_some() {
            return Err(Error::Stopped);
        }
        if self.running {
            return Err(Error::Running);
        }
        if self.queue.progress().outstanding_commands == 0 {
            return Err(Error::EmptyQueue);
        }
        self.running = true;
        self.sm.clkdiv_restart();
        self.sm.set_enable(true);
        Ok(())
    }

    /// Drain at most four completion records, without pausing pulse generation.
    /// Returns a completed-segment checkpoint, or a latched terminal report.
    /// This is NOT an instantaneous position or a Hall-edge capture.
    pub fn poll(&mut self) -> Result<Progress, StopReport> {
        if let Some(report) = self.report {
            return Err(report);
        }
        self.collect();
        if self.running {
            match self.relative_pc() {
                program::STARVED..=23 => return Err(self.stop(StopReason::Starved)),
                program::FINISHED..=25 => return Err(self.stop(StopReason::Completed)),
                _ => {}
            }
        }
        Ok(self.queue.progress())
    }

    /// Wait for End or starvation. Usually call after committing End. This does
    /// not refill the queue. Dropping a polled, pending wait aborts execution and
    /// preserves a Cancelled report, retrievable through poll/abort.
    /// Returns NotStarted if called before start (no wait is established).
    pub async fn wait_stopped(&mut self) -> Result<StopReport, Error> {
        if let Err(report) = self.poll() {
            return Ok(report);
        }
        if !self.running {
            return Err(Error::NotStarted);
        }
        let mut guard = CancelGuard {
            driver: self,
            armed: true,
        };
        loop {
            guard.driver.irq.wait().await;
            if let Err(report) = guard.driver.poll() {
                guard.armed = false;
                return Ok(report);
            }
        }
    }

    /// Immediately disable PIO, force STEP low, discard both FIFOs and retain
    /// the emitted-edge count. May truncate a high pulse. Does not decelerate,
    /// engage a brake, disable motor power, or prove the mechanism stopped.
    /// Stop latency includes caller scheduling and register operations; no wall
    /// clock latency is claimed without target measurement.
    pub fn abort(&mut self) -> StopReport {
        self.stop(StopReason::Aborted)
    }

    /// Explicitly arm a new run after stopping. Discards any idle preload and
    /// resets per-run counts. Does not start motion or change DIR immediately.
    /// Retrieve the old StopReport before calling this method.
    pub fn reset(&mut self) -> Result<(), Error> {
        if self.running && self.poll().is_ok() {
            return Err(Error::Running);
        }
        self.initialize();
        self.queue = Queue::new(Direction::Negative);
        self.report = None;
        Ok(())
    }

    fn relative_pc(&self) -> u8 {
        self.sm.get_addr().wrapping_sub(self.program.origin) & 31
    }

    fn collect(&mut self) {
        // Snapshot the backlog: never drain an unbounded concurrently growing FIFO.
        let count = self.sm.rx().level();
        for _ in 0..count {
            self.sm.rx().pull();
            self.queue.acknowledge();
        }
    }

    fn stop(&mut self, reason: StopReason) -> StopReport {
        if let Some(report) = self.report {
            return report;
        }
        self.sm.set_enable(false);
        let pc = self.relative_pc();
        self.collect();
        // SM_RESTART clears delay/stall state, but preserves scratch Y. Record
        // PC first; forced instructions below destroy the execution location.
        self.sm.restart();
        self.sm.set_pins(Level::Low, &[&self.pins.step]);
        // SAFETY: exclusive, disabled SM, RX was drained, no DMA survives an
        // awaited transfer/cancellation guard. Y contains the remaining pulse count.
        unsafe {
            self.sm.exec_instr(
                InstructionOperands::MOV {
                    destination: MovDestination::ISR,
                    op: MovOperation::None,
                    source: MovSource::Y,
                }
                .encode(),
            );
            self.sm.exec_instr(
                InstructionOperands::PUSH {
                    if_full: false,
                    block: false,
                }
                .encode(),
            );
        }
        let remaining = self.sm.rx().pull();
        let report = self.queue.stopped(pc, remaining, reason);
        self.initialize();
        self.report = Some(report);
        report
    }

    fn initialize(&mut self) {
        self.sm.set_enable(false);
        self.sm.clear_fifos();
        self.sm.restart();
        self.sm.set_config(&self.sm_config);
        self.sm.set_pins(Level::Low, &[&self.pins.step]);
        self.flags.clear(SM);
        // SAFETY: owned disabled SM, jump to loaded startup guard, no side effects.
        unsafe {
            self.sm
                .exec_instr(u16::from((self.program.origin + program::START) & 31));
        }
        self.running = false;
    }
}

struct CancelGuard<'a, 'd, P: Instance, const SM: usize> {
    driver: &'a mut PioStepDir<'d, P, SM>,
    armed: bool,
}

impl<P: Instance, const SM: usize> Drop for CancelGuard<'_, '_, P, SM> {
    fn drop(&mut self) {
        if self.armed {
            // Nested Embassy DMA Transfer is dropped/aborted before this guard.
            self.driver.stop(StopReason::Cancelled);
        }
    }
}

impl<P: Instance, const SM: usize> Drop for PioStepDir<'_, P, SM> {
    fn drop(&mut self) {
        self.sm.set_enable(false);
        self.sm.restart();
        self.sm.set_pins(Level::Low, &[&self.pins.step]);
        self.sm.clear_fifos();
        self.flags.clear(SM);
    }
}
