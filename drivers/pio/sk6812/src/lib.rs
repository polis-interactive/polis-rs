#![no_std]
//! SK6812-family RGBW signaling for RP2040 and RP2350A boards.
//!
//! Enable exactly one of `rp2040` or `rp235xa` for the hardware driver. With no
//! features, only the color type is public; waveform and timing tests run on the host.
//! See the package README for timing, resource, and cancellation contracts.

#[cfg(all(feature = "rp2040", feature = "rp235xa"))]
compile_error!("select only one MCU feature: rp2040 or rp235xa");

pub use smart_leds::RGBW;

#[cfg(any(feature = "rp2040", feature = "rp235xa", test))]
mod waveform;

#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
mod driver;
#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
pub use driver::{PioSk6812, PioSk6812Program};
