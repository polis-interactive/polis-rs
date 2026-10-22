//! Scrolling text and bars on a 128×32 SSD1306: GP6 SDA, GP7 SCL, address 0x3C.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{bind_interrupts, i2c, peripherals};
use embassy_time::{Duration, Ticker, Timer};
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_7X14},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};
use panic_probe as _;
use polis_ssd1306::{Ssd1306Async, WIDTH};

// ASCII text, including the gap before it repeats. Each 7×14 cell is doubled.
const MESSAGE: &str = "I LOVE DICK  ";
const SCALE: i32 = 2;
const TEXT_HEIGHT: i32 = 28;
const TEXT_WIDTH: i32 = MESSAGE.len() as i32 * 7 * SCALE;
const PADDING: i32 = 2;
const BAR_HEIGHT: i32 = 2;
// Half-screen bars separated by quarter-screen gaps: 64 pixels on, 32 off.
const BAR_WIDTH: i32 = WIDTH as i32 / 2;
const BAR_GAP: i32 = WIDTH as i32 / 4;
const BAR_PERIOD: i32 = BAR_WIDTH + BAR_GAP;
const FRAME_MS: u64 = 33;

// Scale the font and wrap the banner at pixel resolution without a second buffer.
struct ScrollingText<'a, D> {
    display: &'a mut D,
    offset: i32,
}

impl<D> OriginDimensions for ScrollingText<'_, D> {
    fn size(&self) -> Size {
        Size::new((TEXT_WIDTH / SCALE) as u32, (TEXT_HEIGHT / SCALE) as u32)
    }
}

impl<D: DrawTarget<Color = BinaryColor>> DrawTarget for ScrollingText<'_, D> {
    type Color = BinaryColor;
    type Error = D::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let offset = self.offset;
        self.display.draw_iter(
            pixels
                .into_iter()
                .flat_map(|Pixel(point, color)| {
                    (0..SCALE).flat_map(move |dy| {
                        (0..SCALE).map(move |dx| {
                            Pixel(
                                Point::new(
                                    (point.x * SCALE + dx - offset).rem_euclid(TEXT_WIDTH),
                                    point.y * SCALE + dy,
                                ),
                                color,
                            )
                        })
                    })
                })
                .filter(|Pixel(point, _)| {
                    point.x < WIDTH as i32 && (0..TEXT_HEIGHT).contains(&point.y)
                }),
        )
    }
}

bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<peripherals::I2C1>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let mut config = i2c::Config::default();
    config.frequency = 400_000;
    config.sda_pullup = false;
    config.scl_pullup = false;
    let bus = i2c::I2c::new_async(p.I2C1, p.PIN_7, p.PIN_6, Irqs, config);
    // Example assumes a module with onboard power-on reset and pull-ups to 3.3 V.
    Timer::after_millis(100).await;
    let mut display = Ssd1306Async::new(bus);
    display.init().await.unwrap();

    let style = MonoTextStyle::new(&FONT_7X14, BinaryColor::On);
    let mut ticker = Ticker::every(Duration::from_millis(FRAME_MS));
    let mut text_offset = 0;
    let mut bar_offset = 0;
    loop {
        display.clear_buffer();
        Text::with_baseline(MESSAGE, Point::zero(), style, Baseline::Top)
            .draw(&mut ScrollingText {
                display: &mut display,
                offset: text_offset,
            })
            .unwrap();

        // Rows 28–29 stay blank; rows 30–31 contain a continuous bar/gap pattern.
        display
            .draw_iter((0..WIDTH as i32).flat_map(|x| {
                let color = if (x + bar_offset) % BAR_PERIOD < BAR_WIDTH {
                    BinaryColor::On
                } else {
                    BinaryColor::Off
                };
                (0..BAR_HEIGHT)
                    .map(move |dy| Pixel(Point::new(x, TEXT_HEIGHT + PADDING + dy), color))
            }))
            .unwrap();
        display.flush().await.unwrap();

        // One pixel left per frame, with independent wrap periods and no reset jump.
        text_offset = (text_offset + 1) % TEXT_WIDTH;
        bar_offset = (bar_offset + 1) % BAR_PERIOD;
        ticker.next().await;
    }
}
