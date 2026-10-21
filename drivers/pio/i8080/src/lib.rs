#![no_std]
#![deny(missing_docs)]
//! Write-only 8-bit 8080 display bus: active-low CS/WR, high DC for data.
//!
//! Select `rp2040` or `rp235xa` for the PIO/DMA implementation. No chip is
//! selected by default; timing configuration remains available on the host.
//! Controller commands, reset, power, and RD (held high) belong to the caller.

#[cfg(all(feature = "rp2040", feature = "rp235xa"))]
compile_error!("select only one MCU feature: rp2040 or rp235xa");

/// PIO timing configuration. Keep the system clock unchanged after construction.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Maximum PIO instruction frequency, not the byte rate. Range: 1..=50 MHz.
    /// An integer divider rounds the frequency down. Each byte takes six PIO
    /// cycles (plus FIFO stalls): one setup cycle, two WR-low cycles, at least
    /// three WR-high cycles. Default: 12 MHz, at most 2 MB/s.
    pub max_clock_hz: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_clock_hz: 12_000_000,
        }
    }
}

/// Invalid timing or pin configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// Zero clock, clock above 50 MHz, or divider outside 1..=65535.
    Clock,
    /// D0..D7 must be consecutive ascending GPIOs; WR must be separate.
    Pins,
}

impl Config {
    /// Calculate a whole-number divider, preventing fractional-clock jitter.
    ///
    /// Actual PIO frequency is `system_hz / divider`; byte rate is at most
    /// `system_hz / (6 * divider)`. The peripheral must accept the resulting
    /// setup, hold, and pulse durations; this is not automatic timing discovery.
    pub fn divider(self, system_hz: u32) -> Result<u16, ConfigError> {
        if system_hz == 0 || self.max_clock_hz == 0 || self.max_clock_hz > 50_000_000 {
            return Err(ConfigError::Clock);
        }
        u16::try_from(system_hz.div_ceil(self.max_clock_hz)).map_err(|_| ConfigError::Clock)
    }
}

#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
mod driver;
#[cfg(any(feature = "rp2040", feature = "rp235xa", test))]
mod waveform;
#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
pub use driver::{Pins, PioI8080, PioI8080Rgb332, PioI8080Rgb332Pipeline, Rgb332Converter};
