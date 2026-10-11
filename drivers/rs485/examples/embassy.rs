//! Send "hello" through a THVD1420: GP18 DI, GP20 RO, GP19 tied DE + /RE.
//! Simplified from SlateCode/bin/test-rs485; see PROVENANCE.md.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{
    bind_interrupts,
    clocks::ClockConfig,
    gpio::{Level, Output},
    peripherals, pio,
};
use embassy_time::{Delay, Timer};
use embedded_hal::digital::PinState;
use panic_probe as _;
use pio_uart::{PioUart, Programs};
use rs485::{Config, Rs485};

bind_interrupts!(struct Irqs {
    PIO1_IRQ_0 => pio::InterruptHandler<peripherals::PIO1>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let mut config = embassy_rp::config::Config::default();
    config.clocks = ClockConfig::system_freq(128_000_000).unwrap();
    let p = embassy_rp::init(config);
    let direction = Output::new(p.PIN_19, Level::Low);
    let mut pio = pio::Pio::new(p.PIO1, Irqs);
    let programs = Programs::new(&mut pio.common);
    // This wrapper's flush waits for wire completion before DE is released.
    let serial = PioUart::new(
        &mut pio.common,
        pio.sm0,
        pio.sm1,
        p.PIN_18,
        p.PIN_20,
        &programs,
        1_000_000,
    )
    .unwrap();
    let mut bus = Rs485::new(
        serial,
        direction,
        Delay,
        Config {
            tx_level: PinState::High,
            enable_us: 12,
            hold_us: 1,
            receive_us: 12,
        },
    );
    bus.recover().await.unwrap(); // Drain stale TX bytes with DE inactive.
    loop {
        bus.send(b"hello\r\n").await.unwrap();
        defmt::info!("Sent hello; back in receive mode");
        Timer::after_secs(1).await;
    }
}
