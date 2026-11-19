#![no_std]
#![warn(missing_docs)]
//! Bounded, DMA-fed STEP/DIR execution, independent of motion planning.
//!
//! Host builds expose timing validation, segments, and reports. Select exactly
//! one MCU feature for `PioStepDir`. See the README for
//! timing equations, queue ownership, cancellation, and qualification limits.

#[cfg(all(feature = "rp2040", feature = "rp235xa"))]
compile_error!("select only one MCU feature: rp2040 or rp235xa");

#[cfg(any(feature = "rp2040", feature = "rp235xa", test))]
mod queue;
mod timing;
pub use timing::{Config, Direction, Error, Progress, Segment, StopReason, StopReport};

#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
mod driver;
#[cfg(any(feature = "rp2040", feature = "rp235xa", test))]
mod program;
#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
pub use driver::{Pins, PioStepDir};
