//! Async I²C driver for 128×64 SH1106 OLED modules with a two-column RAM offset.
//!
//! Drawing updates an allocation-free framebuffer; [`Sh1106Async::flush`] sends
//! it to the panel. The only required dependencies are embedded-hal-async and
//! embedded-graphics-core. See the package README for power/reset requirements,
//! error recovery and the opt-in Mousefood/Ratatui example.
#![no_std]
#![deny(missing_docs)]

use embedded_graphics_core::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::BinaryColor,
};
use embedded_hal_async::i2c::I2c;

/// Visible width in pixels (the controller has 132 RAM columns).
pub const WIDTH: usize = 128;
/// Visible height in pixels.
pub const HEIGHT: usize = 64;
/// Framebuffer size in bytes, arranged as eight pages of 128 columns.
pub const BUFFER_SIZE: usize = WIDTH * HEIGHT / 8;
const PAGES: usize = HEIGHT / 8;
const COL_OFFSET: u8 = 2;

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

/// Buffered, allocation-free driver for a 128×64 SH1106 module.
///
/// The caller owns power sequencing, hardware reset, scheduling and bus recovery.
/// Supply an owned bus, `&mut bus`, or an async shared-bus device. Use only one
/// driver for a given panel; page addressing spans multiple I²C transactions.
pub struct Sh1106Async<I2C> {
    i2c: I2C,
    address: Address,
    buf: [u8; BUFFER_SIZE],
}

impl<I2C> Sh1106Async<I2C> {
    /// Construct with address 0x3C and a blank framebuffer. Performs no I/O.
    pub fn new(i2c: I2C) -> Self {
        Self::with_address(i2c, Address::Primary)
    }

    /// Construct with an explicit address and a blank framebuffer. Performs no I/O.
    pub fn with_address(i2c: I2C, address: Address) -> Self {
        Self {
            i2c,
            address,
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

    /// Access the bus for caller-managed recovery.
    ///
    /// If device configuration is changed or lost, call [`Self::init`] again.
    pub fn bus_mut(&mut self) -> &mut I2C {
        &mut self.i2c
    }

    /// Release the bus without changing the display's state.
    pub fn into_inner(self) -> I2C {
        self.i2c
    }
}

impl<I2C: I2c> Sh1106Async<I2C> {
    async fn cmd(&mut self, cmd: u8) -> Result<(), I2C::Error> {
        self.i2c.write(self.address as u8, &[0x00, cmd]).await
    }

    async fn cmd2(&mut self, cmd: u8, arg: u8) -> Result<(), I2C::Error> {
        self.i2c.write(self.address as u8, &[0x00, cmd, arg]).await
    }

    /// Initialize a module using its internal DC-DC converter, upload the current
    /// framebuffer, then enable the display. Existing drawn pixels are preserved.
    ///
    /// Supply power and complete hardware reset before calling this method; it
    /// does not drive reset or wait for power stabilization. Configuration uses
    /// the original module's remapped segment/COM orientation and 0x7F contrast.
    /// An error or cancellation may leave partial configuration; recover the bus
    /// as needed and repeat `init()` before drawing. Underlying I²C errors are
    /// returned unchanged, with no automatic retries.
    pub async fn init(&mut self) -> Result<(), I2C::Error> {
        self.cmd(0xAE).await?; // Display off while configuring the converter.
        self.cmd2(0xD5, 0x80).await?; // Clock divider.
        self.cmd2(0xA8, 0x3F).await?; // Multiplex 64.
        self.cmd2(0xD3, 0x00).await?; // Display offset.
        self.cmd(0x40).await?; // Start line 0.
        self.cmd2(0xAD, 0x8B).await?; // SH1106 DC-DC on (not SSD1306 0x8D/0x14).
        self.cmd(0xA1).await?; // Segment remap.
        self.cmd(0xC8).await?; // Remapped COM scan direction.
        self.cmd2(0xDA, 0x12).await?; // Alternative COM configuration.
        self.cmd2(0x81, 0x7F).await?; // Contrast.
        self.cmd2(0xD9, 0x22).await?; // Pre-charge/discharge periods.
        self.cmd2(0xDB, 0x20).await?; // VCOM deselect level.
        self.cmd(0xA4).await?; // Follow display RAM.
        self.cmd(0xA6).await?; // Normal (not inverted).
        self.flush().await?; // Do not show uninitialized controller RAM.
        self.cmd(0xAF).await // Display on.
    }

    /// Upload all eight pages, even if no pixels have changed.
    ///
    /// Each page uses three address writes followed by a 129-byte I²C write:
    /// one data-control byte and 128 pixel bytes, targeting RAM columns 2..=129.
    /// The bus must support writes of that length. Drawing itself performs no I/O.
    ///
    /// Errors/cancellation can leave a partially updated panel but never discard
    /// local pixels. After caller-managed bus recovery, retry the full flush;
    /// every page address is set again. If the panel reset, repeat `init()` instead.
    /// Cancellation safety of an in-flight transaction belongs to the bus.
    pub async fn flush(&mut self) -> Result<(), I2C::Error> {
        let mut page_buf = [0u8; WIDTH + 1];
        page_buf[0] = 0x40;
        for page in 0..PAGES {
            self.cmd(0xB0 | page as u8).await?;
            self.cmd(COL_OFFSET & 0x0F).await?;
            self.cmd(0x10 | (COL_OFFSET >> 4)).await?;
            let start = page * WIDTH;
            page_buf[1..].copy_from_slice(&self.buf[start..start + WIDTH]);
            self.i2c.write(self.address as u8, &page_buf).await?;
        }
        Ok(())
    }
}

impl<I2C> OriginDimensions for Sh1106Async<I2C> {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl<I2C> DrawTarget for Sh1106Async<I2C> {
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
