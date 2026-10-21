//! Logic-analyzer fixture; leave LCD disconnected. Not a panel initializer.
#![no_std]
#![no_main]
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{
    bind_interrupts, dma,
    gpio::{Level, Output},
    peripherals,
    pio::{self, Pio},
};
use embassy_time::Timer;
use panic_probe as _;
use polis_pio_i8080::{Config as BusConfig, Pins, PioI8080};
bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<peripherals::PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<peripherals::DMA_CH0>;
});
#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let cs = Output::new(p.PIN_12, Level::High);
    let dc = Output::new(p.PIN_11, Level::High);
    let Pio {
        mut common,
        sm0,
        irq0,
        irq_flags,
        ..
    } = Pio::new(p.PIO0, Irqs);
    let pins = Pins {
        data: [
            common.make_pio_pin(p.PIN_2),
            common.make_pio_pin(p.PIN_3),
            common.make_pio_pin(p.PIN_4),
            common.make_pio_pin(p.PIN_5),
            common.make_pio_pin(p.PIN_6),
            common.make_pio_pin(p.PIN_7),
            common.make_pio_pin(p.PIN_8),
            common.make_pio_pin(p.PIN_9),
        ],
        wr: common.make_pio_pin(p.PIN_10),
        dc,
        cs,
    };
    let mut bus = PioI8080::new(
        &mut common,
        sm0,
        dma::Channel::new(p.DMA_CH0, Irqs),
        irq0,
        irq_flags,
        pins,
        BusConfig::default(),
    )
    .unwrap();
    loop {
        bus.write_command(0x2c, &[]).await;
        bus.write_data(&[0x00, 0x01, 0x80, 0x55, 0xaa, 0xff]).await;
        Timer::after_millis(100).await;
    }
}
