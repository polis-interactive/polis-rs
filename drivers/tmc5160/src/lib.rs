//! Async TMC5160/A SPI configuration and diagnostics for external STEP/DIR.
//!
//! Portable, allocation-free, with no selected MCU or executor. Owns one
//! `SpiDevice<u8>` and a delay provider. Configure mode 3, MSB-first and suitable
//! clock/CS edge timing in the backend. The driver supplies the CS-high interval.
//! STEP, DIR, DRV_ENN, brakes, motion, UART and OTP programming are not owned here.
//!
//! Errors/cancellation poison the transport until explicit backend recovery.
//! No generic SpiDevice cancellation guarantee exists: a dropped transaction may
//! leave CS asserted or DMA active. `recover_with` must first restore the backend.
//! Configuration is never cached as hardware truth or automatically replayed.
#![no_std]
#![deny(missing_docs)]
#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod config;
pub mod registers;
pub use config::{BoardConfig, CurrentSettings, StepDirConfig};
use embedded_hal_async::{delay::DelayNs, spi::SpiDevice};
use registers::{Readable, Register, Writable};

/// Invalid configuration, detected before any corresponding SPI write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigError {
    /// A board resistance/current ceiling was zero.
    Board,
    /// Current settings were calculated for a different board.
    BoardMismatch,
    /// Current/delay is outside the supported or representable range.
    Current,
    /// Reserved bits were set.
    ReservedBits {
        /// Register address.
        address: u8,
    },
    /// Unsupported field encoding or field combination.
    InvalidField {
        /// Register address.
        address: u8,
    },
    /// A zero CS-high delay was requested.
    Timing,
}

/// Transport, validation or configuration precondition failure. No automatic retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<E> {
    /// Original SPI-device error, including CS errors when provided by the backend.
    Spi(E),
    /// An earlier error or cancelled operation requires explicit backend recovery.
    RecoveryRequired,
    /// Invalid configuration; no associated write occurred.
    Config(ConfigError),
    /// IOIN version was not the documented 0x30 (also catches stuck-low/high MISO).
    UnexpectedVersion(u8),
    /// SD_MODE was low; select external STEP/DIR in hardware.
    ExternalStepDirRequired,
    /// DRV_ENN was low; assert external disable before configuring.
    DriverDisableRequired,
}

/// Minimum idle interval between CS assertions. The backend owns SCK/CS edge timing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timing {
    cs_high_ns: u32,
}
impl Timing {
    /// Conservative 1 µs interval for the chip's internal oscillator.
    /// An external slow clock may require a longer interval (>2*tCLK + 10 ns).
    pub const INTERNAL_CLOCK: Self = Self { cs_high_ns: 1_000 };
    /// Explicit minimum CS-high interval; caller checks against the chip clock.
    pub const fn new(cs_high_ns: u32) -> Result<Self, ConfigError> {
        if cs_high_ns == 0 {
            return Err(ConfigError::Timing);
        }
        Ok(Self { cs_high_ns })
    }
    /// Configured minimum idle interval in nanoseconds.
    pub const fn cs_high_ns(self) -> u32 {
        self.cs_high_ns
    }
}

/// Status byte returned during a datagram (not an acknowledgement of that write).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpiStatus(u8);
impl SpiStatus {
    /// Complete status byte, including motion-controller bits not interpreted here.
    pub const fn bits(self) -> u8 {
        self.0
    }
    /// Reset status was active when this datagram started.
    pub const fn reset(self) -> bool {
        self.0 & 1 != 0
    }
    /// Driver-error status was active.
    pub const fn driver_error(self) -> bool {
        self.0 & 2 != 0
    }
    /// StallGuard status was active.
    pub const fn stalled(self) -> bool {
        self.0 & 4 != 0
    }
    /// Standstill status was active.
    pub const fn standstill(self) -> bool {
        self.0 & 8 != 0
    }
}

/// One pipelined register read, retaining status from both datagrams.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadResponse<R> {
    /// Requested register data from the second transaction.
    pub value: R,
    /// Status returned while submitting the read request; its data is discarded.
    pub request_status: SpiStatus,
    /// Status returned while fetching the requested data.
    pub response_status: SpiStatus,
}

/// Progress of the most recent configure call; never a hardware-state guarantee.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationProgress {
    /// No complete configuration recorded, or invalidated by recovery/manual writes.
    Unknown,
    /// Precondition reads have started; no configuration write attempted yet.
    Checking,
    /// Complete writes before the next/uncertain write. Available after cancellation.
    Writing {
        /// Number of successfully completed configuration datagrams.
        completed: u8,
        /// Register whose write is next, in progress, or failed with an uncertain effect.
        address: u8,
    },
    /// All configuration writes completed, without proof of physical application.
    Complete,
}

/// Exclusive logical owner of one TMC5160/A SPI pipeline.
///
/// Sharing the underlying bus with other CS lines is fine. Two independent owners
/// for this same chip may interleave pipelined reads and are unsupported.
pub struct Tmc5160<SPI, DELAY> {
    spi: SPI,
    delay: DELAY,
    board: BoardConfig,
    timing: Timing,
    recovery_required: bool,
    progress: ConfigurationProgress,
}

impl<SPI, DELAY> Tmc5160<SPI, DELAY> {
    /// Take an idle backend with CS high. Performs no I/O and selects no motor current.
    pub const fn new(spi: SPI, delay: DELAY, board: BoardConfig, timing: Timing) -> Self {
        Self {
            spi,
            delay,
            board,
            timing,
            recovery_required: false,
            progress: ConfigurationProgress::Unknown,
        }
    }
    /// Explicitly selected board characteristics.
    pub const fn board(&self) -> BoardConfig {
        self.board
    }
    /// Whether the backend must be recovered before further I/O.
    pub const fn recovery_required(&self) -> bool {
        self.recovery_required
    }
    /// Last configuration progress, including partial progress on cancellation.
    pub const fn configuration_progress(&self) -> ConfigurationProgress {
        self.progress
    }
    /// Release resources. This does not recover SPI/DMA, deassert CS or disable the motor.
    pub fn into_parts(self) -> (SPI, DELAY) {
        (self.spi, self.delay)
    }
}

impl<SPI: SpiDevice<u8>, DELAY: DelayNs> Tmc5160<SPI, DELAY> {
    async fn exchange(
        &mut self,
        address: u8,
        data: u32,
    ) -> Result<(SpiStatus, u32), Error<SPI::Error>> {
        if self.recovery_required {
            return Err(Error::RecoveryRequired);
        }
        // Set before every await. A dropped future cannot accidentally unlock reuse.
        self.recovery_required = true;
        // Before every datagram, including the first and following a separate API call.
        self.delay.delay_ns(self.timing.cs_high_ns).await;
        let bytes = data.to_be_bytes();
        let mut frame = [address, bytes[0], bytes[1], bytes[2], bytes[3]];
        self.spi
            .transfer_in_place(&mut frame)
            .await
            .map_err(Error::Spi)?;
        self.recovery_required = false;
        Ok((
            SpiStatus(frame[0]),
            u32::from_be_bytes([frame[1], frame[2], frame[3], frame[4]]),
        ))
    }

    /// Read with two independent CS transactions. Reissues the requested address
    /// for the second datagram so no unrelated clear-on-read register is touched.
    pub async fn read<R: Readable>(&mut self) -> Result<ReadResponse<R>, Error<SPI::Error>> {
        let (request_status, _) = self.exchange(R::ADDRESS, 0).await?;
        let (response_status, bits) = self.exchange(R::ADDRESS, 0).await?;
        Ok(ReadResponse {
            value: R::from_read(bits),
            request_status,
            response_status,
        })
    }

    /// Write a complete typed register. Low-level writes do not enforce board/motor
    /// current limits or external ENN state; use configure for checked initialization.
    /// A successful SPI transfer does not prove the chip accepted the write.
    pub async fn write<R: Writable>(&mut self, value: R) -> Result<SpiStatus, Error<SPI::Error>> {
        value.validate().map_err(Error::Config)?;
        if self.recovery_required {
            return Err(Error::RecoveryRequired);
        }
        self.progress = ConfigurationProgress::Unknown;
        self.exchange(R::ADDRESS | 0x80, value.bits())
            .await
            .map(|v| v.0)
    }

    /// Explicitly acknowledge selected GSTAT flags with write-one-to-clear semantics.
    /// Capture diagnostics first. Acknowledging reset does not restore configuration.
    pub async fn clear_status(
        &mut self,
        flags: registers::Gstat,
    ) -> Result<SpiStatus, Error<SPI::Error>> {
        if flags.bits() & !7 != 0 {
            return Err(Error::Config(ConfigError::ReservedBits { address: 1 }));
        }
        self.exchange(0x81, flags.bits()).await.map(|v| v.0)
    }

    /// Recover using caller-supplied backend repair, then wait with CS high.
    ///
    /// The callback MUST stop/drain pending transfers, deassert CS and restore an
    /// idle bus before returning success. Generic SpiDevice cannot do this itself.
    /// If the backend exposes no repair API, release/rebuild it instead. Failed or
    /// cancelled recovery stays blocked. No configuration or write is replayed.
    pub async fn recover_with(
        &mut self,
        recover: impl AsyncFnOnce(&mut SPI) -> Result<(), SPI::Error>,
    ) -> Result<(), Error<SPI::Error>> {
        self.recovery_required = true;
        self.progress = ConfigurationProgress::Unknown;
        recover(&mut self.spi).await.map_err(Error::Spi)?;
        self.delay.delay_ns(self.timing.cs_high_ns).await;
        self.recovery_required = false;
        Ok(())
    }

    /// Apply a complete STEP/DIR configuration while external DRV_ENN stays high.
    ///
    /// Validates the entire configuration before I/O, checks IOIN version, SD_MODE
    /// and DRV_ENN, disables the chopper first, then applies current/configuration
    /// and writes the desired CHOPCONF last. Caller must keep ENN high and STEP
    /// inactive throughout; the initial read cannot enforce later pin levels.
    /// Does not clear faults, start pulses, perform automatic tuning or enable ENN.
    /// Errors/cancellation leave partial progress accessible; do not resume from
    /// that count. Recover SPI, inspect faults and explicitly reapply as appropriate.
    pub async fn configure(
        &mut self,
        config: &StepDirConfig,
    ) -> Result<SpiStatus, Error<SPI::Error>> {
        config.validate(self.board).map_err(Error::Config)?;
        if self.recovery_required {
            return Err(Error::RecoveryRequired);
        }
        self.progress = ConfigurationProgress::Checking;
        let input = self.read::<registers::Ioin>().await?.value;
        if input.version() != 0x30 {
            return Err(Error::UnexpectedVersion(input.version()));
        }
        if !input.external_step_dir() {
            return Err(Error::ExternalStepDirRequired);
        }
        if !input.driver_disabled() {
            return Err(Error::DriverDisableRequired);
        }
        // Values are all validated before the first transfer; no hidden cache/RMW.
        let mut writes = [None; 13];
        writes[0] = Some((
            registers::Chopconf::ADDRESS,
            config.chopper.disabled().bits(),
        ));
        writes[1] = config
            .short
            .map(|v| (registers::ShortConf::ADDRESS, v.bits()));
        writes[2] = config
            .bridge
            .map(|v| (registers::DrvConf::ADDRESS, v.bits()));
        writes[3] = Some((
            registers::GlobalScaler::ADDRESS,
            config.current.global_scaler().bits(),
        ));
        writes[4] = Some((
            registers::IholdIrun::ADDRESS,
            config.current.ihold_irun().bits(),
        ));
        writes[5] = Some((registers::Tpowerdown::ADDRESS, config.power_down.bits()));
        writes[6] = Some((registers::Tpwmthrs::ADDRESS, config.pwm_threshold.bits()));
        writes[7] = Some((registers::Tcoolthrs::ADDRESS, config.cool_threshold.bits()));
        writes[8] = Some((registers::Thigh::ADDRESS, config.high_threshold.bits()));
        writes[9] = Some((registers::Coolconf::ADDRESS, config.cool.bits()));
        writes[10] = Some((registers::Pwmconf::ADDRESS, config.pwm.bits()));
        writes[11] = Some((registers::Gconf::ADDRESS, config.global.bits()));
        writes[12] = Some((registers::Chopconf::ADDRESS, config.chopper.bits()));
        let mut status = SpiStatus(0);
        for (completed, (address, value)) in writes.into_iter().flatten().enumerate() {
            self.progress = ConfigurationProgress::Writing {
                completed: completed as u8,
                address,
            };
            status = self.exchange(address | 0x80, value).await?.0;
        }
        self.progress = ConfigurationProgress::Complete;
        Ok(status)
    }
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
