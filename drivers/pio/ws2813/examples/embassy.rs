//! Blink one eight-pixel WS2813 RGB chain on GP2.
//! Simplified from SlateCode/bin/test-ws2813; see PROVENANCE.md.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{bind_interrupts, dma, peripherals, pio};
use embassy_time::Timer;
use panic_probe as _;
use pio_ws2813::{PioWs2813, PioWs2813Program, RGB8};

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<peripherals::PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<peripherals::DMA_CH0>;
});

const LED_COUNT: usize = 8; // Set to the number of LEDs in your chain.

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let mut pio = pio::Pio::new(p.PIO0, Irqs);
    let program = PioWs2813Program::new(&mut pio.common);
    let mut strip = PioWs2813::<_, 0, LED_COUNT>::new(
        &mut pio.common,
        pio.sm0,
        p.DMA_CH0,
        Irqs,
        p.PIN_2,
        &program,
    );
    loop {
        strip.write(&[RGB8::new(16, 0, 0); LED_COUNT]).await;
        Timer::after_millis(500).await;
        strip.write(&[RGB8::new(0, 0, 0); LED_COUNT]).await;
        Timer::after_millis(500).await;
    }
}
