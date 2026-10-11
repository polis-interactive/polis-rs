//! TMP102/TMP112 temperature: GP6 SDA, GP7 SCL, ADD0 grounded (0x48).
//! Simplified from SlateCode/bin/test-tmp1x2; see PROVENANCE.md.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{bind_interrupts, i2c, peripherals};
use embassy_time::Timer;
use panic_probe as _;
use tmp1x2::{Address, Config, ConversionRate, Tmp1x2};

bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<peripherals::I2C1>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    // Use external pull-ups to 3.3 V.
    let mut config = i2c::Config::default();
    config.frequency = 400_000;
    config.sda_pullup = false;
    config.scl_pullup = false;
    let bus = i2c::I2c::new_async(p.I2C1, p.PIN_7, p.PIN_6, Irqs, config);
    let mut sensor = Tmp1x2::new(bus, Address::Ground);
    sensor
        .configure(Config {
            conversion_rate: ConversionRate::Hz1,
            ..Default::default()
        })
        .await
        .unwrap();

    loop {
        // Also allows the initial conversion to complete before the first read.
        Timer::after_secs(1).await;
        let temperature = sensor.read_temperature().await.unwrap();
        defmt::info!("Temperature: {} milli-Celsius", temperature.milli_celsius());
    }
}
