#![no_std]
//! Single-controller PIO I²C, adapted from Raspberry Pi's pico-examples.
//!
//! Enable exactly one of `rp2040` or `rp235xa` for the hardware API. The application
//! provides Embassy's time driver; the BSP supplies pins and a state machine. See the README for
//! timing, pull-ups, timeout and cancellation requirements.

#[cfg(all(feature = "rp2040", feature = "rp235xa"))]
compile_error!("select only one MCU feature: rp2040 or rp235xa");

pub use embedded_hal::i2c::{NoAcknowledgeSource, Operation};

/// Bus operation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// A target did not acknowledge an address or a written data byte.
    NoAcknowledge(NoAcknowledgeSource),
    /// The whole transaction/recovery exceeded its configured deadline.
    Timeout,
    /// An earlier timeout or cancelled future requires explicit bus recovery.
    RecoveryRequired,
    /// Only unshifted 7-bit addresses are supported.
    AddressOutOfRange(u8),
    /// Empty read operations are not supported. Empty writes probe an address.
    InvalidReadBufferLength,
}

impl embedded_hal::i2c::Error for Error {
    fn kind(&self) -> embedded_hal::i2c::ErrorKind {
        match *self {
            Self::NoAcknowledge(source) => embedded_hal::i2c::ErrorKind::NoAcknowledge(source),
            _ => embedded_hal::i2c::ErrorKind::Other,
        }
    }
}

#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
mod driver;
#[cfg(any(feature = "rp2040", feature = "rp235xa", test))]
mod protocol;
#[cfg(any(feature = "rp2040", feature = "rp235xa", test))]
mod waveform;
#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
pub use driver::{Async, Blocking, Config, ConfigError, I2c, Mode};

#[cfg(test)]
extern crate std;
