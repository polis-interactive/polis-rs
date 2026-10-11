#![no_std]
//! WS2813B-B RGB signaling for legacy Slate LED chains.
//!
//! Enable exactly one of `rp2040` or `rp235xa` for the hardware driver.
//! With no features, the color type is public and waveform tests run on the host.
//! See the package README for timing, resource, and cancellation contracts.

#[cfg(all(feature = "rp2040", feature = "rp235xa"))]
compile_error!("select only one MCU feature: rp2040 or rp235xa");

pub use smart_leds::RGB8;

#[cfg(any(feature = "rp2040", feature = "rp235xa", test))]
mod waveform;

#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
mod driver;
#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
pub use driver::{PioWs2813, PioWs2813Program};
