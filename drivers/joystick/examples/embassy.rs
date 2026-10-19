//! Pico/Pico 2 example: joystick X/Y on GP26/GP27, common ground.
//! The calibration below is illustrative; measure endpoints/center for your hardware.
#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_rp::{
    adc::{self, Adc, Channel},
    bind_interrupts,
    gpio::Pull,
};
use embassy_time::{Duration, Timer, with_timeout};
use polis_joystick::{Calibration, Joystick, rp::RpAxes};
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs { ADC_IRQ_FIFO => adc::InterruptHandler; });

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let mut adc = Adc::new(p.ADC, Irqs, Default::default());
    let mut x = Channel::new_pin(p.PIN_26, Pull::None);
    let mut y = Channel::new_pin(p.PIN_27, Pull::None);
    let mut source = RpAxes {
        adc: &mut adc,
        x: &mut x,
        y: &mut y,
    };
    let joystick = Joystick::new([Calibration::new(0, 2048, 4095, 50, false).unwrap(); 2]);
    loop {
        match with_timeout(Duration::from_millis(20), joystick.sample(&mut source)).await {
            Ok(Ok(reading)) => {
                defmt::info!("raw={:?}, position={:?}", reading.raw, reading.position)
            }
            _ => defmt::warn!("on-chip joystick ADC failure"),
        }
        Timer::after_millis(20).await;
    }
}
