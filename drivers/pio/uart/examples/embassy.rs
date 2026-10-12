//! UART echo: GP18 TX, GP20 RX, 1 Mbaud 8N1, 3.3 V logic.
//! Setup simplified from SlateCode/bin/test-rs485; see PROVENANCE.md.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{bind_interrupts, clocks::ClockConfig, peripherals, pio};
use embedded_io_async::{Read, Write};
use panic_probe as _;
use polis_pio_uart::{PioUart, Programs};

bind_interrupts!(struct Irqs {
    PIO1_IRQ_0 => pio::InterruptHandler<peripherals::PIO1>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let mut config = embassy_rp::config::Config::default();
    // Exact integer divider: 128 MHz / (8 * 1 Mbaud). Keep this clock fixed.
    config.clocks = ClockConfig::system_freq(128_000_000).unwrap();
    let p = embassy_rp::init(config);
    let mut pio = pio::Pio::new(p.PIO1, Irqs);
    let programs = Programs::new(&mut pio.common);
    let mut serial = PioUart::new(
        &mut pio.common,
        pio.sm0,
        pio.sm1,
        p.PIN_18,
        p.PIN_20,
        &programs,
        1_000_000,
    )
    .unwrap();

    // Send one byte at a time from a USB-UART adapter and wait for each echo.
    // The PIO RX FIFO is small; this is not a sustained-throughput example.
    let mut byte = [0];
    loop {
        serial.read_exact(&mut byte).await.unwrap();
        serial.write_all(&byte).await.unwrap();
        serial.flush().await.unwrap(); // Wait through the final stop bit.
        defmt::info!("Echoed: {}", byte[0]);
    }
}
