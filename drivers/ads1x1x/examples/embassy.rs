//! ADS1015 AIN0 sampling: GP26 SDA, GP27 SCL, ADDR grounded (0x48).
//! Simplified from ControlCode/ads1015-rs/example/embassy-rp; see PROVENANCE.md.
#![no_std]
#![no_main]

use ads1x1x::{Address, Ads1015, Channel, Config, model::DataRate12Bit};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{bind_interrupts, i2c, peripherals};
use embassy_time::{Delay, Timer};
use panic_probe as _;

bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<peripherals::I2C1>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    // External pull-ups to 3.3 V are required on SDA and SCL.
    let mut config = i2c::Config::default();
    config.frequency = 100_000;
    config.sda_pullup = false;
    config.scl_pullup = false;
    let bus = i2c::I2c::new_async(p.I2C1, p.PIN_27, p.PIN_26, Irqs, config);
    let mut adc = Ads1015::new(bus, Address::Ground);
    adc.configure_async(Config {
        data_rate: DataRate12Bit::Sps128,
        ..Default::default()
    })
    .await
    .unwrap();

    loop {
        // Trigger and poll one conversion; no ALERT/RDY pin is needed.
        let raw = adc
            .read_one_shot_async(Channel::Single0, &mut Delay, 200)
            .await
            .unwrap();
        defmt::info!("AIN0: {} counts", raw);
        Timer::after_secs(1).await;
    }
}
