//! Mirror ABX00110 button levels onto its LEDs: GP6 SDA, GP7 SCL, address 0x3e.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{bind_interrupts, i2c, peripherals};
use embassy_time::Timer;
use panic_probe as _;
use polis_modulino_buttons::{Error, ModulinoButtons};

bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<peripherals::I2C1>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    // Supply 3.3 V and external SDA/SCL pull-ups to 3.3 V (e.g. 4.7 kohm).
    let mut config = i2c::Config::default();
    config.frequency = 100_000;
    config.sda_pullup = false;
    config.scl_pullup = false;
    let bus = i2c::I2c::new_async(p.I2C1, p.PIN_7, p.PIN_6, Irqs, config);
    let mut buttons = ModulinoButtons::new(bus);
    let mut previous = None;

    // Example startup allowance, not a guaranteed firmware startup bound.
    Timer::after_millis(100).await;
    loop {
        match buttons.read_buttons().await {
            Ok(state) => {
                if previous != Some(state) {
                    defmt::info!("Buttons: A={} B={} C={}", state.a, state.b, state.c);
                    previous = Some(state);
                }
                if buttons.set_leds(state.a, state.b, state.c).await.is_err() {
                    defmt::warn!("LED write failed; retrying on the next poll");
                }
            }
            Err(Error::I2c(_)) => defmt::warn!("Button read failed; check wiring and address"),
            Err(Error::UnexpectedDevice { pinstrap_id }) => {
                defmt::warn!("Unexpected Modulino identifier: {=u8:#x}", pinstrap_id);
            }
        }
        // This demonstrates levels only: a 20 ms polling interval is not debounce.
        Timer::after_millis(20).await;
    }
}
