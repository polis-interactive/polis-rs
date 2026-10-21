//! Async I²C driver for 128×32 SSD1306 OLED modules using the upstream async controller implementation.
//!
//! Drawing updates an allocation-free framebuffer; [`Ssd1306Async::flush`] sends
//! it to the panel. Controller commands use the `ssd1306` crate
//! with its async feature; graphics traits use embedded-graphics-core. See the
//! package README for power/reset requirements, recovery and the UI example.
#![no_std]
#![deny(missing_docs)]

use embedded_graphics_core::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::BinaryColor,
};
use embedded_hal_async::i2c::I2c;

/// Visible width in pixels.
pub const WIDTH: usize = 128;
/// Visible height in pixels.
pub const HEIGHT: usize = 32;
/// Framebuffer size in bytes, arranged as four pages of 128 columns.
pub const BUFFER_SIZE: usize = WIDTH * HEIGHT / 8;

use ssd1306::{
    I2CDisplayInterface,
    command::AddrMode,
    mode::BasicMode,
    prelude::{DisplayRotation, DisplaySize128x32, I2CInterface},
};

/// Errors from the upstream display interface. I²C failures become `BusWriteError`.
pub use display_interface::DisplayError;

type Controller<I2C> = ssd1306::Ssd1306Async<I2CInterface<I2C>, DisplaySize128x32, BasicMode>;

/// Unshifted seven-bit I²C address selected by the module's SA0 pin.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum Address {
    /// SA0 low: 0x3C (the usual module default).
    #[default]
    Primary = 0x3C,
    /// SA0 high: 0x3D.
    Secondary = 0x3D,
}

/// Buffered, allocation-free driver for a 128×32 SSD1306 module.
///
/// The caller owns power sequencing, hardware reset, scheduling and bus recovery.
/// Supply an owned bus, `&mut bus`, or an async shared-bus device. Use only one
/// driver for a given panel; window addressing spans multiple I²C transactions.
pub struct Ssd1306Async<I2C> {
    controller: Controller<I2C>,
    buf: [u8; BUFFER_SIZE],
}

impl<I2C: I2c> Ssd1306Async<I2C> {
    /// Construct with address 0x3C and a blank framebuffer. Performs no I/O.
    pub fn new(i2c: I2C) -> Self {
        Self::with_address(i2c, Address::Primary)
    }

    /// Construct with an explicit address and a blank framebuffer. Performs no I/O.
    pub fn with_address(i2c: I2C, address: Address) -> Self {
        Self {
            controller: ssd1306::Ssd1306Async::new(
                I2CDisplayInterface::new_custom_address(i2c, address as u8),
                DisplaySize128x32,
                DisplayRotation::Rotate0,
            ),
            buf: [0; BUFFER_SIZE],
        }
    }

    /// Clear the local framebuffer. Call [`Self::flush`] to update the panel.
    pub fn clear_buffer(&mut self) {
        self.buf.fill(0);
    }

    /// Read page-major bytes: index `(y / 8) * WIDTH + x`, bit `y % 8`.
    pub fn buffer(&self) -> &[u8; BUFFER_SIZE] {
        &self.buf
    }

    /// Release the bus without changing the display's state.
    pub fn into_inner(self) -> I2C {
        self.controller.release().release()
    }
}

impl<I2C: I2c> Ssd1306Async<I2C> {
    /// Configure a 128×32 module with its internal charge pump, then upload pixels.
    ///
    /// Power and hardware reset must already be complete. This method uses the
    /// upstream horizontal-addressing initialization and preserves local pixels.
    /// Upstream enables the display before the upload, so stale controller RAM
    /// can briefly be visible during startup. No reset or power delay is supplied.
    ///
    /// On failure or cancellation, recover the bus as needed and repeat `init()`.
    /// Underlying I²C errors are mapped to [`DisplayError::BusWriteError`].
    pub async fn init(&mut self) -> Result<(), DisplayError> {
        self.controller
            .init_with_addr_mode(AddrMode::Horizontal)
            .await?;
        self.flush().await
    }

    /// Upload the entire 512-byte framebuffer, including unchanged pixels.
    ///
    /// Restores horizontal addressing and resets the column/page window to
    /// columns 0..=127 and pages 0..=3 on every
    /// call. The upstream I²C interface sends 16 data bytes per write, prefixed
    /// by 0x40 (17 bytes total). No dirty state is discarded before I/O succeeds.
    ///
    /// Errors/cancellation can leave partial panel updates but preserve all local
    /// pixels. Retry after bus recovery; if configuration was lost, repeat `init()`.
    /// Bus cancellation safety and timeouts belong to the caller/bus implementation.
    pub async fn flush(&mut self) -> Result<(), DisplayError> {
        self.controller.set_addr_mode(AddrMode::Horizontal).await?;
        self.controller
            .set_draw_area((0, 0), (WIDTH as u8, HEIGHT as u8))
            .await?;
        self.controller.draw(&self.buf).await
    }
}

impl<I2C> OriginDimensions for Ssd1306Async<I2C> {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl<I2C> DrawTarget for Ssd1306Async<I2C> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            if coord.x < 0 || coord.x >= WIDTH as i32 || coord.y < 0 || coord.y >= HEIGHT as i32 {
                continue;
            }
            let x = coord.x as usize;
            let y = coord.y as usize;
            let idx = (y / 8) * WIDTH + x;
            if color.is_on() {
                self.buf[idx] |= 1 << (y % 8);
            } else {
                self.buf[idx] &= !(1 << (y % 8));
            }
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.buf.fill(if color.is_on() { 0xFF } else { 0 });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
