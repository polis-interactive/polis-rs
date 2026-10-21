use super::*;
use embedded_graphics_core::pixelcolor::{PixelColor, RgbColor, raw::RawU8};
use lcd_async::{interface::InterfaceKind, raw_framebuf::IntoRawBytes};

/// Caller-owned RGB332 framebuffer size: 76,800 bytes (75 KiB).
pub const RGB332_BUFFER_SIZE: usize = WIDTH * HEIGHT;

/// A byte interface with an explicit RGB332 pixel-transfer operation.
///
/// Ordinary [`Interface`] operations must remain byte-exact, including command
/// parameters. Only `send_rgb332` expands packed `RRRGGGBB` pixels to big-endian
/// RGB565, replicating component bits (`abc -> abcab`, `abc -> abcabc`,
/// `ab -> ababa`). Completion and cancellation are transport responsibilities.
/// The single-SM and two-SM pipeline adapters implement this using PIO + DMA;
/// ordinary SPI interfaces deliberately do not implement it. Implementations
/// must accept a full framebuffer slice, bounding DMA chunks internally.
#[allow(async_fn_in_trait)]
pub trait Rgb332Interface: Interface<Word = u8> {
    /// Transfer one RGB565 pixel for each supplied RGB332 byte.
    async fn send_rgb332(&mut self, pixels: &[u8]) -> Result<(), Self::Error>;
}

impl<T: Rgb332Interface> Rgb332Interface for &mut T {
    async fn send_rgb332(&mut self, pixels: &[u8]) -> Result<(), Self::Error> {
        T::send_rgb332(self, pixels).await
    }
}

// Keep expansion private to the framebuffer driver: it cannot accidentally be
// passed to Ili9341Async (whose pixel slices already contain RGB565).
struct Expand<DI>(DI);
impl<DI: Rgb332Interface> Interface for Expand<DI> {
    type Word = u8;
    type Error = DI::Error;
    const KIND: InterfaceKind = DI::KIND;

    async fn send_command(&mut self, command: u8, args: &[u8]) -> Result<(), Self::Error> {
        self.0.send_command(command, args).await
    }

    async fn send_data_slice(&mut self, pixels: &[u8]) -> Result<(), Self::Error> {
        self.0.send_rgb332(pixels).await
    }
}

/// A 75 KiB RGB332 framebuffer with asynchronous RGB565 output.
///
/// Accepts the same `DrawTarget<Color = Rgb565>` drawing calls as
/// [`Ili9341Async`], truncating to the top 3/3/2 component bits when drawing.
/// Mousefood therefore needs no new color adapter. Native RGB332 renderers can
/// write [`Self::buffer_mut`] directly. Flush performs no CPU pixel conversion
/// or staging copy. The display stays in RGB565 mode.
pub struct Ili9341Rgb332<'a, DI: Rgb332Interface, RST: OutputPin> {
    controller: Display<Expand<DI>, ILI9341Rgb565, RST>,
    buffer: &'a mut [u8; RGB332_BUFFER_SIZE],
}

impl<'a, DI: Rgb332Interface, RST: OutputPin> Ili9341Rgb332<'a, DI, RST> {
    /// Initialize landscape RGB565 output and upload the existing RGB332 frame.
    ///
    /// Supply zeroed storage for an initial black frame. Power, backlight,
    /// reset delays, failure/cancellation and borrowed-resource recovery follow
    /// [`Ili9341Async::new`]. The interface must explicitly support expansion.
    pub async fn new(
        interface: DI,
        reset: RST,
        buffer: &'a mut [u8; RGB332_BUFFER_SIZE],
        config: Config,
        delay: &mut impl DelayNs,
    ) -> Result<Self, InitError<DI::Error, RST::Error>> {
        let rotation = match config.landscape {
            Landscape::Normal => Rotation::Deg90,
            Landscape::Flipped => Rotation::Deg270,
        };
        let controller = Builder::new(ILI9341Rgb565, Expand(interface))
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

    /// Upload the full RGB332 framebuffer using one address window.
    ///
    /// Supplies 76,800 bytes to the expanding transport, which bounds its own
    /// DMA chunks. The LCD still receives 153,600 RGB565 bytes. Keeping one
    /// window lets the two-SM transport overlap conversion and output across
    /// staging buffers. Failure/cancellation preserves local
    /// pixels; after bus recovery retry the full flush. Reinitialize after a
    /// panel reset. There is no dirty tracking or tear-free guarantee.
    pub async fn flush(&mut self) -> Result<(), DI::Error> {
        self.flush_rows(0..HEIGHT).await
    }

    /// Upload a full-width row range using one address window.
    ///
    /// `0..HEIGHT` uploads a full frame; `y..y + 16` uploads a wipe band.
    /// Bounds are clipped to the framebuffer; empty/reversed ranges do no I/O.
    /// The transport can overlap chunks across the entire range.
    /// No dirty tracking or tear synchronization is
    /// performed. After cancellation/recovery, retry the same range.
    pub async fn flush_rows(&mut self, rows: core::ops::Range<usize>) -> Result<(), DI::Error> {
        let start = rows.start.min(HEIGHT);
        let end = rows.end.min(HEIGHT);
        if start >= end {
            return Ok(());
        }
        self.controller
            .show_raw_data(
                0,
                start as u16,
                WIDTH as u16,
                (end - start) as u16,
                &self.buffer[start * WIDTH..end * WIDTH],
            )
            .await
    }

    /// Clear local pixels to black without display I/O.
    pub fn clear_buffer(&mut self) {
        self.buffer.fill(0);
    }

    /// Inspect row-major packed `RRRGGGBB` pixels, one byte per pixel.
    pub fn buffer(&self) -> &[u8; RGB332_BUFFER_SIZE] {
        self.buffer
    }

    /// Edit native RGB332 pixels without conversion or display I/O.
    pub fn buffer_mut(&mut self) -> &mut [u8; RGB332_BUFFER_SIZE] {
        self.buffer
    }

    /// Return the idle interface, reset pin and original framebuffer.
    pub fn release(self) -> (DI, RST, &'a mut [u8; RGB332_BUFFER_SIZE]) {
        let (interface, _, reset) = self.controller.release();
        (
            interface.0,
            reset.expect("reset pin supplied by constructor"),
            self.buffer,
        )
    }

    fn frame(&mut self) -> RawFrameBuf<PackedRgb332, &mut [u8]> {
        RawFrameBuf::new(&mut self.buffer[..], WIDTH, HEIGHT)
    }
}

#[derive(Clone, Copy, PartialEq)]
struct PackedRgb332(u8);
impl PixelColor for PackedRgb332 {
    type Raw = RawU8;
}
impl IntoRawBytes for PackedRgb332 {
    const BYTES_PER_PIXEL: usize = 1;
    type Raw = [u8; 1];
    fn into_raw_bytes(self) -> [u8; 1] {
        [self.0]
    }
}
impl From<Rgb565> for PackedRgb332 {
    fn from(color: Rgb565) -> Self {
        Self(((color.r() >> 2) << 5) | ((color.g() >> 3) << 2) | (color.b() >> 3))
    }
}

impl<DI: Rgb332Interface, RST: OutputPin> OriginDimensions for Ili9341Rgb332<'_, DI, RST> {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl<DI: Rgb332Interface, RST: OutputPin> DrawTarget for Ili9341Rgb332<'_, DI, RST> {
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.frame()
            .draw_iter(pixels.into_iter().map(|Pixel(p, c)| Pixel(p, c.into())))
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.frame().clear(color.into())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        self.frame().fill_solid(area, color.into())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        self.frame()
            .fill_contiguous(area, colors.into_iter().map(Into::into))
    }
}
