use embassy_rp::{
    Peri,
    gpio::Pull,
    pio::{
        Common, Config as SmConfig, Direction, FifoJoin, Instance, LoadError, LoadedProgram,
        PioPin, ShiftDirection, StateMachine,
    },
};
use fixed::types::U24F8;

use crate::{Position, program};

/// Program shared by up to four state machines in one PIO block.
/// Uses instructions 0..=26; computed jumps require origin zero.
pub struct PioEncoderProgram<'d, P: Instance> {
    program: LoadedProgram<'d, P>,
}

impl<'d, P: Instance> PioEncoderProgram<'d, P> {
    /// Load once per PIO block. Fails if instructions 0..=26 are occupied.
    pub fn new(common: &mut Common<'d, P>) -> Result<Self, LoadError> {
        Ok(Self {
            program: common.try_load_program(&program::program())?,
        })
    }
}

/// Application-owned electrical and sampling configuration.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// PIO clock divider in 1..=65536 (8 fractional bits).
    /// Default 1 runs at clk_sys. Maximum interval between samples is ten
    /// PIO cycles; allow synchronizer and signal-integrity margin as in README.
    pub clock_divider: U24F8,
    /// Pull for both contacts; default Up for switches wired to ground.
    pub pull: Pull,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            clock_divider: U24F8::from_bits(256),
            pull: Pull::Up,
        }
    }
}

impl Config {
    /// Conservative upper bound on system clock ticks between input samples,
    /// including fractional-divider phase (not GPIO synchronization latency).
    /// Panics for dividers outside 1..=65536, as does driver construction.
    pub fn max_sample_interval_sys_ticks(&self) -> u32 {
        self.validate();
        (u64::from(self.clock_divider.to_bits()) * u64::from(program::MAX_SAMPLE_CYCLES))
            .div_ceil(256) as u32
            + 1
    }

    fn validate(&self) {
        assert!(
            self.clock_divider >= 1 && self.clock_divider <= 65536,
            "PIO divider must be in 1..=65536"
        );
    }
}

/// One continuously running electrical transition counter.
///
/// Does not choose an executor, time driver, DMA channel, polling interval, or
/// mechanical direction. Dropping it disables its state machine. Retain the
/// driver and tracker across transport reconnects to preserve the baseline.
pub struct PioEncoder<'d, 'p, P: Instance, const SM: usize> {
    sm: StateMachine<'d, P, SM>,
    _program: &'p PioEncoderProgram<'d, P>,
}

impl<'d, 'p, P: Instance, const SM: usize> PioEncoder<'d, 'p, P, SM> {
    /// Start at zero at the first PIO phase observation, without synthetic motion.
    ///
    /// `pin_b_low` must be the numerically lower GPIO, and `pin_a_high` must be
    /// exactly the next GPIO. "Low/high" denotes pin number, not voltage.
    /// Panics on nonadjacent/reversed pins, GPIOs above 29 (RP2350A scope), or a
    /// divider outside 1..=65536. Inputs must be electrically settled during startup.
    pub fn new(
        common: &mut Common<'d, P>,
        mut sm: StateMachine<'d, P, SM>,
        pin_b_low: Peri<'d, impl PioPin>,
        pin_a_high: Peri<'d, impl PioPin>,
        program: &'p PioEncoderProgram<'d, P>,
        config: Config,
    ) -> Self {
        assert_eq!(
            pin_b_low.pin().checked_add(1),
            Some(pin_a_high.pin()),
            "encoder pins must be consecutive, B lower and A higher"
        );
        assert!(
            pin_a_high.pin() <= 29,
            "only RP2040/RP2350A GPIOs are supported"
        );
        config.validate();
        sm.set_enable(false);
        sm.clear_fifos();
        sm.restart();
        let mut b = common.make_pio_pin(pin_b_low);
        let mut a = common.make_pio_pin(pin_a_high);
        b.set_pull(config.pull);
        a.set_pull(config.pull);
        b.set_schmitt(true);
        a.set_schmitt(true);
        // Keep the normal two-flop synchronizer enabled on both contacts.
        b.set_input_sync_bypass(false);
        a.set_input_sync_bypass(false);
        sm.set_pin_dirs(Direction::In, &[&b, &a]);
        let mut cfg = SmConfig::default();
        cfg.use_program(&program.program, &[]);
        cfg.set_in_pins(&[&b, &a]);
        cfg.fifo_join = FifoJoin::RxOnly;
        cfg.shift_in.direction = ShiftDirection::Left;
        cfg.shift_out.direction = ShiftDirection::Right;
        cfg.shift_in.auto_fill = false;
        cfg.shift_out.auto_fill = false;
        cfg.shift_in.threshold = 32;
        cfg.shift_out.threshold = 32;
        cfg.clock_divider = config.clock_divider;
        sm.set_config(&cfg);
        // SAFETY: owned SM is disabled, program is loaded at origin zero, and
        // START is its initialization entry. JMP has no pin or FIFO effects.
        unsafe {
            sm.exec_instr(u16::from(program::START));
        }
        sm.clkdiv_restart();
        sm.set_enable(true);
        Self {
            sm,
            _program: program,
        }
    }

    /// Discard the initial FIFO backlog (at most eight words), then await one
    /// newly queued cumulative snapshot. Exactly N+1 FIFO reads; never drain
    /// until empty because the producer may be faster than the CPU.
    ///
    /// The returned count may have been captured just before this call, or may
    /// become stale while this future is descheduled. It is not timestamped or
    /// atomic across encoders. PIO continues counting during waits/cancellation;
    /// the next call discards any remaining backlog. InterruptHandler for the PIO
    /// block must be bound through Embassy's Pio::new for the wait to wake.
    pub async fn snapshot(&mut self) -> Position {
        let rx = self.sm.rx();
        let backlog = rx.level();
        for _ in 0..backlog {
            rx.pull();
        }
        Position(rx.wait_pull().await)
    }
}
