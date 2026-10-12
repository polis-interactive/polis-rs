//! Read a TMP102/TMP112 register using PIO I2C: GP6 SDA, GP7 SCL.
//! Simplified from SlateCode/bin/test-tmp1x2's PIO setup; see PROVENANCE.md.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{bind_interrupts, peripherals, pio};
use embassy_time::Timer;
use panic_probe as _;
use polis_pio_i2c::{Config, I2c};

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<peripherals::PIO0>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let mut pio = pio::Pio::new(p.PIO0, Irqs);
    // External pull-ups to 3.3 V; ADD0 grounded selects address 0x48.
    let mut config = Config::default();
    config.frequency = 400_000;
    let mut bus = I2c::new(&mut pio.common, pio.sm0, p.PIN_7, p.PIN_6, config);
    loop {
        let mut config_register = [0; 2];
        // Standard async HAL transaction: pointer write, repeated START, read.
        embedded_hal_async::i2c::I2c::write_read(&mut bus, 0x48, &[0x01], &mut config_register)
            .await
            .unwrap();
        defmt::info!("TMP configuration: {=[u8]:x}", config_register);
        Timer::after_secs(1).await;
    }
}
