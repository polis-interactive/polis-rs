//! Link-only fixture for two concurrent chains.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{bind_interrupts, dma, peripherals, pio};
use panic_probe as _;
use pio_ws2813::{PioWs2813, PioWs2813Program, RGB8};

#[cfg(feature = "rp2040")]
bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<peripherals::PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<peripherals::DMA_CH0>, dma::InterruptHandler<peripherals::DMA_CH1>;
});
#[cfg(feature = "rp235xa")]
bind_interrupts!(struct Irqs {
    PIO2_IRQ_0 => pio::InterruptHandler<peripherals::PIO2>;
    DMA_IRQ_0 => dma::InterruptHandler<peripherals::DMA_CH0>, dma::InterruptHandler<peripherals::DMA_CH1>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    #[cfg(feature = "rp2040")]
    let mut pio = pio::Pio::new(p.PIO0, Irqs);
    #[cfg(feature = "rp235xa")]
    let mut pio = pio::Pio::new(p.PIO2, Irqs);
    let program = PioWs2813Program::new(&mut pio.common);
    let left =
        PioWs2813::<_, 0, 63>::new(&mut pio.common, pio.sm0, p.DMA_CH0, Irqs, p.PIN_2, &program);
    let bottom =
        PioWs2813::<_, 1, 70>::new(&mut pio.common, pio.sm1, p.DMA_CH1, Irqs, p.PIN_3, &program);
    let black = RGB8::new(0, 0, 0);
    let mut outputs = led_output::led_outputs!(left, bottom);
    loop {
        outputs.write(&[black; 133]).await.unwrap();
        embassy_time::Timer::after_millis(20).await;
    }
}
