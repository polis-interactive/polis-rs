#![no_std]
//! Embassy PIO UART wrapper with short reads and a conservative physical flush.
//! Uses Embassy's existing TX/RX programs unchanged; no RS-485 direction policy.

#[cfg(all(feature = "rp2040", feature = "rp235xa"))]
compile_error!("select only one MCU feature: rp2040 or rp235xa");

#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
mod driver;
#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
pub use driver::{PioUart, Programs};

#[cfg(any(feature = "rp2040", feature = "rp235xa", test))]
mod timing;
#[cfg(any(feature = "rp2040", feature = "rp235xa", test))]
pub use timing::ConfigError;
