use embassy_rp::{bind_interrupts, i2c, peripherals};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};
use embedded_hal_async::i2c::I2c;
use polis_ssd1306::Ssd1306Async;
use polis_xca954xa::{Address, SharedMux, Xca9548a};

pub type Bus = i2c::I2c<'static, peripherals::I2C1, i2c::Async>;
pub type Mux = SharedMux<CriticalSectionRawMutex, Bus>;

bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<peripherals::I2C1>;
});

pub fn init_mux() -> Mux {
    let p = embassy_rp::init(Default::default());
    let mut config = i2c::Config::default();
    config.frequency = 400_000;
    config.sda_pullup = false;
    config.scl_pullup = false;
    let bus = i2c::I2c::new_async(p.I2C1, p.PIN_7, p.PIN_6, Irqs, config);
    SharedMux::new(Xca9548a::new(bus, Address::DEFAULT))
}

// Drawing is local to this display's framebuffer; only flush acquires the bus.
pub fn draw_frame<B: I2c>(display: &mut Ssd1306Async<B>, label: &str, frame: u32) {
    display.clear_buffer();
    Text::with_baseline(
        label,
        Point::new(0, 0),
        MonoTextStyle::new(&FONT_6X10, BinaryColor::On),
        Baseline::Top,
    )
    .draw(display)
    .unwrap();
    Rectangle::new(Point::new((frame % 113) as i32, 18), Size::new(16, 10))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(display)
        .unwrap();
}
