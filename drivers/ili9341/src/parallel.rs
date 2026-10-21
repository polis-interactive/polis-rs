//! RP PIO + DMA adapter for an 8-bit 8080 display connection.
//!
//! Enable `rp2040` or `rp235xa`. RD must be held high; reset remains a separate
//! argument to [`crate::Ili9341Async::new`]. See the `parallel` example for wiring.

use core::convert::Infallible;
use embassy_rp::pio::{Common, Instance};
use lcd_async::interface::{Interface, InterfaceKind};

pub use polis_pio_i8080::{
    Config as BusConfig, ConfigError, Pins, PioI8080, PioI8080Rgb332, PioI8080Rgb332Pipeline,
    Rgb332Converter,
};

/// RGB332 adapter that overlaps conversion and LCD output on two state machines.
///
/// Wraps [`PioI8080Rgb332Pipeline`]; use with [`crate::Ili9341Rgb332`].
pub struct PioRgb332PipelineInterface<
    'b,
    'd,
    P: Instance,
    const SM: usize,
    C: Instance,
    const CONVERT: usize,
> {
    bus: PioI8080Rgb332Pipeline<'b, 'd, P, SM, C, CONVERT>,
}

impl<'b, 'd, P: Instance, const SM: usize, C: Instance, const CONVERT: usize>
    PioRgb332PipelineInterface<'b, 'd, P, SM, C, CONVERT>
{
    /// Wrap a configured pipeline without sending commands.
    pub fn new(bus: PioI8080Rgb332Pipeline<'b, 'd, P, SM, C, CONVERT>) -> Self {
        Self { bus }
    }

    /// Return the idle pipeline without changing panel memory/configuration.
    pub fn release(self) -> PioI8080Rgb332Pipeline<'b, 'd, P, SM, C, CONVERT> {
        self.bus
    }
}

impl<P: Instance, const SM: usize, C: Instance, const CONVERT: usize> Interface
    for PioRgb332PipelineInterface<'_, '_, P, SM, C, CONVERT>
{
    type Word = u8;
    type Error = Infallible;
    const KIND: InterfaceKind = InterfaceKind::Parallel8Bit;

    async fn send_command(&mut self, command: u8, args: &[u8]) -> Result<(), Self::Error> {
        self.bus.write_command(command, args).await;
        Ok(())
    }

    async fn send_data_slice(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        self.bus.write_data(data).await;
        Ok(())
    }
}

impl<P: Instance, const SM: usize, C: Instance, const CONVERT: usize> crate::Rgb332Interface
    for PioRgb332PipelineInterface<'_, '_, P, SM, C, CONVERT>
{
    async fn send_rgb332(&mut self, pixels: &[u8]) -> Result<(), Self::Error> {
        self.bus.write_rgb332(pixels).await;
        Ok(())
    }
}

/// Thin `lcd-async` adapter. Initialization and pixel handling stay upstream;
/// the owned bus handles CS, DC, WR timing, DMA completion, and cancellation.
pub struct PioInterface<'d, P: Instance, const SM: usize> {
    bus: PioI8080<'d, P, SM>,
}

/// Byte-exact controller interface with an explicit PIO RGB332 pixel path.
///
/// Use with [`crate::Ili9341Rgb332`] for a 75 KiB framebuffer. The wrapped bus
/// owns one SM, IRQ and DMA channel, and 31 instruction slots on one PIO block.
pub struct PioRgb332Interface<'d, P: Instance, const SM: usize> {
    bus: PioI8080Rgb332<'d, P, SM>,
}

impl<'d, P: Instance, const SM: usize> PioRgb332Interface<'d, P, SM> {
    /// Add expansion to an existing bus; requires 24 more free PIO instructions.
    pub fn new(bus: PioI8080<'d, P, SM>, common: &mut Common<'d, P>) -> Self {
        Self {
            bus: PioI8080Rgb332::new(bus, common),
        }
    }

    /// Return the idle bus with RGB332 support; panel configuration is unchanged.
    pub fn release(self) -> PioI8080Rgb332<'d, P, SM> {
        self.bus
    }
}

impl<P: Instance, const SM: usize> Interface for PioRgb332Interface<'_, P, SM> {
    type Word = u8;
    type Error = Infallible;
    const KIND: InterfaceKind = InterfaceKind::Parallel8Bit;

    async fn send_command(&mut self, command: u8, args: &[u8]) -> Result<(), Self::Error> {
        self.bus.write_command(command, args).await;
        Ok(())
    }

    async fn send_data_slice(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        self.bus.write_data(data).await;
        Ok(())
    }
}

impl<P: Instance, const SM: usize> crate::Rgb332Interface for PioRgb332Interface<'_, P, SM> {
    async fn send_rgb332(&mut self, pixels: &[u8]) -> Result<(), Self::Error> {
        self.bus.write_rgb332(pixels).await;
        Ok(())
    }
}

impl<'d, P: Instance, const SM: usize> PioInterface<'d, P, SM> {
    /// Wrap a configured 8080 bus without sending commands.
    pub fn new(bus: PioI8080<'d, P, SM>) -> Self {
        Self { bus }
    }

    /// Return the idle bus. Panel memory/configuration is unchanged.
    pub fn release(self) -> PioI8080<'d, P, SM> {
        self.bus
    }
}

impl<P: Instance, const SM: usize> Interface for PioInterface<'_, P, SM> {
    type Word = u8;
    type Error = Infallible;
    const KIND: InterfaceKind = InterfaceKind::Parallel8Bit;

    async fn send_command(&mut self, command: u8, args: &[u8]) -> Result<(), Self::Error> {
        self.bus.write_command(command, args).await;
        Ok(())
    }

    async fn send_data_slice(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        self.bus.write_data(data).await;
        Ok(())
    }
}
