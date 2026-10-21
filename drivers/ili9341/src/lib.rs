//! Buffered async ILI9341 driver for 320×240 landscape displays.
//!
//! [`Ili9341Async`] stores RGB565; [`Ili9341Rgb332`] halves framebuffer storage
//! with RGB332 and requires a transport that expands pixels to RGB565.
//!
//! Reuses `lcd-async` for controller initialization and async display I/O.
//! Implements synchronous embedded-graphics drawing into caller-owned memory,
//! followed by an explicit asynchronous [`Ili9341Async::flush`]. No allocator,
//! executor, or MCU is selected by ordinary library builds.
#![no_std]
#![deny(missing_docs)]

#[cfg(all(feature = "rp2040", feature = "rp235xa"))]
compile_error!("select only one MCU feature: rp2040 or rp235xa");

#[cfg(any(feature = "rp2040", feature = "rp235xa"))]
pub mod parallel;

mod rgb332;
pub use rgb332::{Ili9341Rgb332, RGB332_BUFFER_SIZE, Rgb332Interface};

use core::convert::Infallible;
use embedded_graphics_core::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::Rgb565,
    primitives::Rectangle,
};
use embedded_hal::digital::OutputPin;
use embedded_hal_async::delay::DelayNs;
use lcd_async::{
    Builder, Display,
    models::ILI9341Rgb565,
    options::{Orientation, Rotation},
    raw_framebuf::RawFrameBuf,
};

pub use lcd_async::{
    InitError,
    interface::{Interface, SpiError, SpiInterface},
    options::{ColorInversion, ColorOrder},
};

/// Landscape width in pixels.
pub const WIDTH: usize = 320;
/// Landscape height in pixels.
pub const HEIGHT: usize = 240;
/// Caller-owned RGB565 framebuffer size: 153,600 bytes (150 KiB).
pub const BUFFER_SIZE: usize = WIDTH * HEIGHT * 2;
const STRIP_ROWS: usize = 8;
const STRIP_BYTES: usize = WIDTH * STRIP_ROWS * 2;

/// Landscape orientation; both variants expose the same 320×240 drawing area.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Landscape {
    /// 90° clockwise relative to the controller's native 240×320 orientation.
    #[default]
    Normal,
    /// 270° clockwise relative to the native orientation.
    Flipped,
}

/// Module-dependent configuration applied during initialization.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Landscape mounting direction. Defaults to [`Landscape::Normal`].
    pub landscape: Landscape,
    /// Panel subpixel order. Defaults to BGR, common on ILI9341 SPI modules.
    pub color_order: ColorOrder,
    /// Panel inversion setting. Defaults to normal (not inverted).
    pub inversion: ColorInversion,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            landscape: Landscape::Normal,
            color_order: ColorOrder::Bgr,
            inversion: ColorInversion::Normal,
        }
    }
}

/// An initialized display and a borrowed, single RGB565 framebuffer.
///
/// Use [`SpiInterface`] with an async `embedded_hal_async::spi::SpiDevice<u8>`
/// and a data/command pin. CS belongs to the SPI device. The caller supplies the
/// reset pin, async delay, and framebuffer storage. For PIO parallel output use
/// `parallel::PioInterface` (requires an MCU feature). In either case, the
/// generic interface must transmit byte-oriented RGB565 data unchanged.
/// Use one driver per panel; addressing and pixels span multiple transactions.
pub struct Ili9341Async<'a, DI: Interface<Word = u8>, RST: OutputPin> {
    controller: Display<DI, ILI9341Rgb565, RST>,
    buffer: &'a mut [u8; BUFFER_SIZE],
}

impl<'a, DI: Interface<Word = u8>, RST: OutputPin> Ili9341Async<'a, DI, RST> {
    /// Reset, initialize RGB565 landscape mode, and upload the supplied pixels.
    ///
    /// The framebuffer is preserved, not cleared. Supply zeroed memory for a
    /// black initial frame. Initialization pulses reset and awaits the upstream
    /// reset/sleep-out delays. Power must already be stable. Backlight control
    /// belongs to the caller; leave it off until this returns to hide startup RAM.
    ///
    /// On error or cancellation, the passed interface and reset handle are
    /// dropped and the panel can be partially configured. Pass borrowed handles
    /// (`&mut interface`, `&mut reset`) if you need to recover them and retry.
    /// The framebuffer is retained by its caller. No automatic retries occur.
    pub async fn new(
        interface: DI,
        reset: RST,
        buffer: &'a mut [u8; BUFFER_SIZE],
        config: Config,
        delay: &mut impl DelayNs,
    ) -> Result<Self, InitError<DI::Error, RST::Error>> {
        let rotation = match config.landscape {
            Landscape::Normal => Rotation::Deg90,
            Landscape::Flipped => Rotation::Deg270,
        };
        // The upstream model describes native 240×320 RAM. Rotate it rather
        // than passing a 320×240 native display_size (which would be invalid).
        let controller = Builder::new(ILI9341Rgb565, interface)
            .reset_pin(reset)
            .orientation(Orientation::new().rotate(rotation))
            .color_order(config.color_order)
            .invert_colors(config.inversion)
            .init(delay)
            .await?;
        let mut display = Self { controller, buffer };
        display.flush().await.map_err(InitError::Interface)?;
        Ok(display)
    }

    /// Upload all pixels in thirty eight-row strips, including unchanged pixels.
    ///
    /// Each strip reissues column/row windows and memory-write-start, then sends
    /// 5,120 contiguous bytes. This bounds each pixel transaction without another
    /// copy or a second framebuffer. The bus must support writes of this size.
    /// Underlying interface errors are returned unchanged.
    ///
    /// Failure or cancellation preserves the local frame but may partially
    /// update the panel. After bus recovery, retry the whole flush; all windows
    /// are set again. If the panel reset, [`Self::release`] and reconstruct it.
    /// In-flight transfer/CS cancellation safety belongs to the SPI device.
    /// There is no dirty tracking, frame synchronization, or tear-free guarantee.
    pub async fn flush(&mut self) -> Result<(), DI::Error> {
        for (strip, bytes) in self.buffer.as_chunks::<STRIP_BYTES>().0.iter().enumerate() {
            self.controller
                .show_raw_data(
                    0,
                    (strip * STRIP_ROWS) as u16,
                    WIDTH as u16,
                    STRIP_ROWS as u16,
                    bytes,
                )
                .await?;
        }
        Ok(())
    }

    /// Clear local pixels to black without performing display I/O.
    pub fn clear_buffer(&mut self) {
        self.buffer.fill(0);
    }

    /// Inspect row-major RGB565 bytes, most significant byte first.
    pub fn buffer(&self) -> &[u8; BUFFER_SIZE] {
        self.buffer
    }

    /// Return the interface, reset pin, and framebuffer without changing hardware.
    ///
    /// A [`SpiInterface`] can in turn be released into its SPI device and DC pin.
    /// Reconstruct with [`Self::new`] after reset/configuration loss. The original
    /// framebuffer is preserved, allowing the same image to be uploaded again.
    pub fn release(self) -> (DI, RST, &'a mut [u8; BUFFER_SIZE]) {
        let (interface, _, reset) = self.controller.release();
        // Every constructor supplies a reset pin and the controller is private.
        (
            interface,
            reset.expect("reset pin supplied by constructor"),
            self.buffer,
        )
    }

    fn frame(&mut self) -> RawFrameBuf<Rgb565, &mut [u8]> {
        RawFrameBuf::new(&mut self.buffer[..], WIDTH, HEIGHT)
    }
}

impl<DI: Interface<Word = u8>, RST: OutputPin> OriginDimensions for Ili9341Async<'_, DI, RST> {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl<DI: Interface<Word = u8>, RST: OutputPin> DrawTarget for Ili9341Async<'_, DI, RST> {
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.frame().draw_iter(pixels)
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.frame().clear(color)
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        self.frame().fill_solid(area, color)
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        self.frame().fill_contiguous(area, colors)
    }
}

#[cfg(test)]
mod tests;
