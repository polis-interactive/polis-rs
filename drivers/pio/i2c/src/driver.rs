use core::{
    future::Future,
    marker::PhantomData,
    task::{Context, Poll, Waker},
};

use embassy_rp::{
    Peri,
    clocks::clk_sys_freq,
    gpio::{Level, Pull},
    pio::{Common, Direction, Instance, LoadedProgram, Pin, PioPin, ShiftDirection, StateMachine},
};
use embassy_time::{Duration, Instant, with_deadline};
use fixed::types::U24F8;

use crate::{
    Error, Operation,
    protocol::{self, Bus},
    waveform,
};
pub use embassy_rp::i2c::{Async, Blocking, Mode};

/// Bus configuration. External pull-ups are expected by default.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Config {
    /// Maximum SCL frequency, 1..=400_000 Hz (subject to divider range).
    pub frequency: u32,
    /// Whole-transaction or bus-recovery deadline, including clock stretching.
    pub timeout: Duration,
    /// Enable SDA's weak internal pull-up; not a substitute for qualified resistors.
    pub sda_pullup: bool,
    /// Enable SCL's weak internal pull-up.
    pub scl_pullup: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            frequency: 100_000,
            timeout: Duration::from_millis(100),
            sda_pullup: false,
            scl_pullup: false,
        }
    }
}

/// Reconfiguration failed; the old configuration remains active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// Rate is zero, exceeds 400 kHz, or is outside the divider range;
    /// alternatively clk_sys is below 32 MHz.
    InvalidFrequency,
    /// A nonzero timeout is required.
    ZeroTimeout,
    /// Recover the bus before reconfiguring it.
    RecoveryRequired,
}

/// Owned PIO I²C controller, following Embassy SPI's mode/API conventions.
/// Uses one state machine and 20 instructions, without DMA or PIO IRQ flags.
/// Only a single controller is supported; there is no arbitration detection.
pub struct I2c<'d, P: Instance, const SM: usize, M: Mode> {
    sm: StateMachine<'d, P, SM>,
    program: LoadedProgram<'d, P>,
    scl: Pin<'d, P>,
    sda: Pin<'d, P>,
    offset: u8,
    config: Config,
    needs_recovery: bool,
    mode: PhantomData<M>,
}

impl<'d, P: Instance, const SM: usize, M: Mode> I2c<'d, P, SM, M> {
    fn new_inner(
        common: &mut Common<'d, P>,
        mut sm: StateMachine<'d, P, SM>,
        scl: Peri<'d, impl PioPin>,
        sda: Peri<'d, impl PioPin>,
        config: Config,
    ) -> Self
    where
        P: 'static,
    {
        let divider = validate_config(&config).expect("invalid PIO I2C config");
        sm.set_enable(false);
        let mut scl = common.make_pio_pin(scl);
        let mut sda = common.make_pio_pin(sda);
        let offset = waveform::scl_offset(sda.pin(), scl.pin());
        let program = common.load_program(&waveform::program(offset));

        sm.set_pin_dirs(Direction::In, &[&scl, &sda]);
        for pin in [&mut scl, &mut sda] {
            pin.set_output_enable_inversion(false);
            pin.set_output_inversion(false);
            pin.set_schmitt(true);
            pin.set_input_sync_bypass(false);
        }
        scl.set_pull(if config.scl_pullup {
            Pull::Up
        } else {
            Pull::None
        });
        sda.set_pull(if config.sda_pullup {
            Pull::Up
        } else {
            Pull::None
        });
        sm.set_pins(Level::Low, &[&scl, &sda]);

        let mut cfg = embassy_rp::pio::Config::default();
        cfg.use_program(&program, &[&scl]);
        cfg.set_out_pins(&[&sda]);
        cfg.set_set_pins(&[&sda]);
        cfg.set_in_pins(&[&sda]);
        cfg.shift_out.direction = ShiftDirection::Left;
        cfg.shift_out.auto_fill = true;
        cfg.shift_out.threshold = 16;
        cfg.shift_in.direction = ShiftDirection::Left;
        cfg.shift_in.auto_fill = false;
        cfg.clock_divider = U24F8::from_bits(divider);
        sm.set_config(&cfg);
        #[cfg(feature = "rp235xa")]
        {
            // RP2350 IN_COUNT also masks WAIT PIN. set_in_pins([sda]) sets
            // it to 1, hiding SCL for every distinct pin pair. Zero exposes
            // all 32 relative inputs, including reversed SCL at offset 31.
            // Only this disabled, owned state machine is changed. GPIO muxes
            // and directions for other pins remain untouched.
            let pio = if core::any::TypeId::of::<P>()
                == core::any::TypeId::of::<embassy_rp::peripherals::PIO0>()
            {
                embassy_rp::pac::PIO0
            } else if core::any::TypeId::of::<P>()
                == core::any::TypeId::of::<embassy_rp::peripherals::PIO1>()
            {
                embassy_rp::pac::PIO1
            } else {
                assert_eq!(
                    core::any::TypeId::of::<P>(),
                    core::any::TypeId::of::<embassy_rp::peripherals::PIO2>()
                );
                embassy_rp::pac::PIO2
            };
            pio.sm(SM).shiftctrl().modify(|w| w.set_in_count(0));
        }

        let mut bus = Self {
            sm,
            program,
            scl,
            sda,
            offset,
            config,
            needs_recovery: false,
            mode: PhantomData,
        };
        bus.reset();
        bus
    }

    /// Stop execution, discard pending data, release both lines and restart at
    /// the command decoder. Only this driver's state machine is affected.
    fn reset(&mut self) {
        self.sm.set_enable(false);
        self.sm.restart();
        self.sm.clear_fifos();
        self.sm.set_pin_dirs(Direction::In, &[&self.scl, &self.sda]);
        // SAFETY: execute JMP to this driver's loaded command decoder with the
        // SM disabled. No other state machine uses this private loaded program.
        unsafe {
            self.sm.exec_instr(u16::from(self.program.wrap.target));
        }
        self.sm.set_enable(true);
    }

    /// Change SCL frequency while idle. `clk_sys` must stay fixed during use.
    pub fn set_frequency(&mut self, frequency: u32) -> Result<(), ConfigError> {
        self.set_config(&Config {
            frequency,
            ..self.config
        })
    }

    /// Update frequency, pull-ups and timeout without reloading instructions.
    pub fn set_config(&mut self, config: &Config) -> Result<(), ConfigError> {
        let divider = validate_config(config)?;
        if self.needs_recovery {
            return Err(ConfigError::RecoveryRequired);
        }
        self.sm.set_enable(false);
        self.sm.set_clock_divider(U24F8::from_bits(divider));
        self.sm.clkdiv_restart();
        self.scl.set_pull(if config.scl_pullup {
            Pull::Up
        } else {
            Pull::None
        });
        self.sda.set_pull(if config.sda_pullup {
            Pull::Up
        } else {
            Pull::None
        });
        self.config = *config;
        self.sm.set_enable(true);
        Ok(())
    }

    /// Execute a complete blocking transaction, including STOP and bus-free time.
    pub fn blocking_transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Error> {
        protocol::validate(address, operations)?;
        if self.needs_recovery {
            return Err(Error::RecoveryRequired);
        }
        let mut session = Session::<_, SM, _, false>::new(self);
        let result = ready(protocol::transaction(&mut session, address, operations));
        session.finish(result)
    }

    /// Read from a 7-bit address, then STOP.
    pub fn blocking_read(&mut self, address: u8, bytes: &mut [u8]) -> Result<(), Error> {
        self.blocking_transaction(address, &mut [Operation::Read(bytes)])
    }

    /// Write to a 7-bit address, then STOP. An empty slice probes the address.
    pub fn blocking_write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Error> {
        self.blocking_transaction(address, &mut [Operation::Write(bytes)])
    }

    /// Write then read using a repeated START (no intervening STOP).
    pub fn blocking_write_read(
        &mut self,
        address: u8,
        write: &[u8],
        read: &mut [u8],
    ) -> Result<(), Error> {
        self.blocking_transaction(
            address,
            &mut [Operation::Write(write), Operation::Read(read)],
        )
    }

    /// Explicit single-controller bus clear: nine clocks and STOP. A target
    /// holding SCL/SDA indefinitely causes Timeout; recovery remains required.
    pub fn blocking_recover(&mut self) -> Result<(), Error> {
        self.reset();
        let mut session = Session::<_, SM, _, false>::new(self);
        let result = ready(protocol::recover(&mut session));
        session.finish(result)
    }
}

impl<'d, P: Instance, const SM: usize> I2c<'d, P, SM, Blocking> {
    /// Create a blocking controller. Panics for invalid configuration/pins or
    /// insufficient instruction memory. Program memory stays allocated in Common.
    pub fn new_blocking(
        common: &mut Common<'d, P>,
        sm: StateMachine<'d, P, SM>,
        scl: Peri<'d, impl PioPin>,
        sda: Peri<'d, impl PioPin>,
        config: Config,
    ) -> Self
    where
        P: 'static,
    {
        Self::new_inner(common, sm, scl, sda, config)
    }
}

impl<'d, P: Instance, const SM: usize> I2c<'d, P, SM, Async> {
    /// Create an async controller. Pio::new must bind Embassy's PIO interrupt
    /// handler. Uses FIFO interrupts, so no DMA channels/IRQ flags are consumed.
    pub fn new(
        common: &mut Common<'d, P>,
        sm: StateMachine<'d, P, SM>,
        scl: Peri<'d, impl PioPin>,
        sda: Peri<'d, impl PioPin>,
        config: Config,
    ) -> Self
    where
        P: 'static,
    {
        Self::new_inner(common, sm, scl, sda, config)
    }

    /// Complete a transaction including STOP. Cancellation releases both lines
    /// and makes the next transaction fail with RecoveryRequired until recover().
    pub async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Error> {
        protocol::validate(address, operations)?;
        if self.needs_recovery {
            return Err(Error::RecoveryRequired);
        }
        let mut session = Session::<_, SM, _, true>::new(self);
        let deadline = session.deadline;
        let result = with_deadline(
            deadline,
            protocol::transaction(&mut session, address, operations),
        )
        .await
        .unwrap_or(Err(Error::Timeout));
        session.finish(result)
    }

    /// Read from a 7-bit address, then STOP.
    pub async fn read(&mut self, address: u8, bytes: &mut [u8]) -> Result<(), Error> {
        self.transaction(address, &mut [Operation::Read(bytes)])
            .await
    }

    /// Write to a 7-bit address, then STOP. Empty writes probe the address.
    pub async fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Error> {
        self.transaction(address, &mut [Operation::Write(bytes)])
            .await
    }

    /// Write then read using a repeated START.
    pub async fn write_read(
        &mut self,
        address: u8,
        write: &[u8],
        read: &mut [u8],
    ) -> Result<(), Error> {
        self.transaction(
            address,
            &mut [Operation::Write(write), Operation::Read(read)],
        )
        .await
    }

    /// Clear a bus after timeout/cancellation. Only valid with a single controller.
    pub async fn recover(&mut self) -> Result<(), Error> {
        self.reset();
        let mut session = Session::<_, SM, _, true>::new(self);
        let deadline = session.deadline;
        let result = with_deadline(deadline, protocol::recover(&mut session))
            .await
            .unwrap_or(Err(Error::Timeout));
        session.finish(result)
    }
}

fn validate_config(config: &Config) -> Result<u32, ConfigError> {
    if config.timeout.as_ticks() == 0 {
        return Err(ConfigError::ZeroTimeout);
    }
    waveform::divider(clk_sys_freq(), config.frequency).ok_or(ConfigError::InvalidFrequency)
}

/// The blocking exchange path never yields. Share transaction logic without an
/// executor, allocation, or a second implementation of the bus state machine.
fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = core::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(result) => result,
        Poll::Pending => unreachable!("blocking PIO exchange yielded"),
    }
}

struct Session<'a, 'd, P: Instance, const SM: usize, M: Mode, const ASYNC: bool> {
    bus: &'a mut I2c<'d, P, SM, M>,
    deadline: Instant,
    complete: bool,
}

impl<'a, 'd, P: Instance, const SM: usize, M: Mode, const ASYNC: bool>
    Session<'a, 'd, P, SM, M, ASYNC>
{
    fn new(bus: &'a mut I2c<'d, P, SM, M>) -> Self {
        let deadline = Instant::now() + bus.config.timeout;
        bus.needs_recovery = true;
        Self {
            bus,
            deadline,
            complete: false,
        }
    }

    fn finish(&mut self, result: Result<(), Error>) -> Result<(), Error> {
        self.complete = result.is_ok() || matches!(result, Err(Error::NoAcknowledge(_)));
        if self.complete {
            self.bus.needs_recovery = false;
        }
        result
    }
}

impl<P: Instance, const SM: usize, M: Mode, const ASYNC: bool> Bus
    for Session<'_, '_, P, SM, M, ASYNC>
{
    fn scl_offset(&self) -> u8 {
        self.bus.offset
    }

    async fn exchange(&mut self, words: &[u16]) -> Result<u32, Error> {
        for &word in words {
            if Instant::now() >= self.deadline {
                return Err(Error::Timeout);
            }
            let word = waveform::fifo_word(word);
            if ASYNC {
                self.bus.sm.tx().wait_push(word).await;
            } else {
                while !self.bus.sm.tx().try_push(word) {
                    if Instant::now() >= self.deadline {
                        return Err(Error::Timeout);
                    }
                    core::hint::spin_loop();
                }
            }
        }
        if ASYNC {
            let sample = self.bus.sm.rx().wait_pull().await;
            if Instant::now() >= self.deadline {
                return Err(Error::Timeout);
            }
            Ok(sample)
        } else {
            loop {
                if Instant::now() >= self.deadline {
                    return Err(Error::Timeout);
                }
                if let Some(sample) = self.bus.sm.rx().try_pull() {
                    return Ok(sample);
                }
                core::hint::spin_loop();
            }
        }
    }
}

impl<P: Instance, const SM: usize, M: Mode, const ASYNC: bool> Drop
    for Session<'_, '_, P, SM, M, ASYNC>
{
    fn drop(&mut self) {
        if !self.complete {
            self.bus.reset();
        }
    }
}

impl<P: Instance, const SM: usize, M: Mode> Drop for I2c<'_, P, SM, M> {
    fn drop(&mut self) {
        self.sm.set_enable(false);
        self.sm.restart();
        self.sm.clear_fifos();
        self.sm.set_pin_dirs(Direction::In, &[&self.scl, &self.sda]);
    }
}

impl<P: Instance, const SM: usize, M: Mode> embedded_hal::i2c::ErrorType for I2c<'_, P, SM, M> {
    type Error = Error;
}

impl<P: Instance, const SM: usize, M: Mode> embedded_hal::i2c::I2c for I2c<'_, P, SM, M> {
    fn transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Error> {
        self.blocking_transaction(address, operations)
    }
}

impl<P: Instance, const SM: usize> embedded_hal_async::i2c::I2c for I2c<'_, P, SM, Async> {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Error> {
        self.transaction(address, operations).await
    }
}
