//! Controller display: GP13 clock, GP15 data, GP14 latch; Pico -> U4 -> U3.
//! Count 00..99 on the two digits, updating once per second.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{
    bind_interrupts, dma,
    gpio::{Level, Output},
    peripherals, pio, spi,
};
use embassy_time::{Delay, Duration, Ticker};
use panic_probe as _;
use polis_74hc595::Hc595;

// Logical segment bits: a, b, c, d, e, f, g. A set bit means requested ON.
const DIGITS: [u8; 10] = [0x3f, 0x06, 0x5b, 0x4f, 0x66, 0x6d, 0x7d, 0x07, 0x7f, 0x6f];

// Physical chain word: bits 0..7 = U3 TOP, bits 8..15 = U4 BOTTOM.
// The PCB distributes each digit across both chips; these are NOT digit bytes.
// Entries correspond to a, b, c, d, e, f, g. Decimal points are not requested.
const ONES: [u16; 7] = [1 << 0, 1 << 1, 1 << 9, 1 << 11, 1 << 10, 1 << 2, 1 << 3];
const TENS: [u16; 7] = [1 << 5, 1 << 4, 1 << 12, 1 << 15, 1 << 14, 1 << 6, 1 << 7];

fn display_frame(count: u8) -> [u8; 2] {
    let ones = DIGITS[usize::from(count % 10)];
    let tens = DIGITS[usize::from((count / 10) % 10)];
    let mut frame = 0u16;
    for segment in 0..7 {
        if ones & (1 << segment) != 0 {
            frame |= ONES[segment];
        }
        if tens & (1 << segment) != 0 {
            frame |= TENS[segment];
        }
    }
    // U3 is farthest from the Pico: transmit TOP before BOTTOM, MSB first each.
    // Active HIGH per the saved PCB: both display common pins connect to GND.
    frame.to_le_bytes()
}

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<peripherals::PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<peripherals::DMA_CH0>, dma::InterruptHandler<peripherals::DMA_CH1>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    // Keep the Controller's RS-485 transmitter disabled throughout this example.
    let _rs485_de = Output::new(p.PIN_1, Level::Low);
    let latch = Output::new(p.PIN_14, Level::Low);
    let mut config = spi::Config::default();
    config.frequency = 1_000_000;
    config.polarity = spi::Polarity::IdleLow;
    config.phase = spi::Phase::CaptureOnFirstTransition;
    // GP13 cannot be a hardware SPI clock, so use Embassy's PIO SPI adapter.
    let mut pio = pio::Pio::new(p.PIO0, Irqs);
    let bus = embassy_rp::pio_programs::spi::Spi::new(
        &mut pio.common,
        pio.sm0,
        p.PIN_13,
        p.PIN_15,
        p.PIN_12, // Unconnected input required by the adapter; received data is discarded.
        p.DMA_CH0,
        p.DMA_CH1,
        Irqs,
        config,
    );
    let mut driver = Hc595::<_, _, _, 2>::new(bus, latch, Delay).unwrap();
    // Both /OE pins are grounded and /MR pins tied high.
    driver.clear().await.expect("initial blank frame failed");
    let mut count = 0u8;
    let mut ticker = Ticker::every(Duration::from_secs(1));
    loop {
        let frame = display_frame(count);
        driver
            .write_outputs(&frame)
            .await
            .expect("display write failed");
        defmt::info!(
            "Count={} TOP={=u8:02x} BOTTOM={=u8:02x}",
            count,
            frame[0],
            frame[1]
        );
        count = (count + 1) % 100;
        ticker.next().await;
    }
}
