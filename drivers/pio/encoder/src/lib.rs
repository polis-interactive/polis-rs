#![no_std]
#![warn(missing_docs)]
//! Cumulative quadrature positions, independent of application service latency.
//!
//! With no features, exposes portable decoding and position policy. Select exactly
//! one of `rp2040` / `rp235xa` for the Embassy RP 0.10 hardware API. See the package
//! README for sampling limits, net-travel reporting, and hardware qualification.

#[cfg(all(feature = "rp2040", feature = "rp235xa"))]
compile_error!("select only one MCU feature: rp2040 or rp235xa");

mod position;
pub use position::{Decoder, Direction, Phase, Position, Reading, Resolution, TrackError, Tracker};

#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
mod driver;
#[cfg(any(feature = "rp2040", feature = "rp235xa", test))]
mod program;
#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
pub use driver::{Config, PioEncoder, PioEncoderProgram};
