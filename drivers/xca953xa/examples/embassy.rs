//! Controller panel wiring: I2C0 GP8/GP9, TCA9534 at 0x20, five buttons/three LEDs.
//! LED anodes use 5 V: output LOW = on; input/high impedance = off.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{bind_interrupts, i2c, peripherals};
use embassy_time::{Duration, Ticker, Timer};
use panic_probe as _;
use polis_xca953xa::{Address, Config, Pin, Xca9534};

const BUTTON_CW: Pin = Pin::P0;
const BUTTON_STOP: Pin = Pin::P1;
const BUTTON_CCW: Pin = Pin::P2;
const BUTTON_MINUS: Pin = Pin::P6;
const BUTTON_PLUS: Pin = Pin::P7;
const LED_CCW: Pin = Pin::P3;
const LED_CW: Pin = Pin::P4;
const LED_STOP: Pin = Pin::P5;

const BUTTON_MASK: u8 = BUTTON_CW.mask()
    | BUTTON_STOP.mask()
    | BUTTON_CCW.mask()
    | BUTTON_MINUS.mask()
    | BUTTON_PLUS.mask(); // 0xc7
const LED_MASK: u8 = LED_CCW.mask() | LED_CW.mask() | LED_STOP.mask(); // 0x38

bind_interrupts!(struct Irqs {
    I2C0_IRQ => i2c::InterruptHandler<peripherals::I2C0>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let mut config = i2c::Config::default();
    config.frequency = 100_000;
    config.sda_pullup = false;
    config.scl_pullup = false;
    // RN9 supplies the board's external 10k SDA/SCL pull-ups to 3.3 V.
    let bus = i2c::I2c::new_async(p.I2C0, p.PIN_9, p.PIN_8, Irqs, config);
    // U5 is TCA9534DWR, with A2/A1/A0 tied to ground; INT is not connected.
    let mut port = Xca9534::new(bus, Address::DEFAULT);
    // Example power-settling allowance; actual sequencing belongs to the board.
    Timer::after_millis(100).await;
    port.init(Config {
        outputs: 0,   // Preload LOW for LED sinking; never drive these pins HIGH.
        polarity: 0,  // Preserve physical levels: pressed buttons read LOW.
        inputs: 0xff, // Buttons input, all LEDs initially off/high impedance.
    })
    .await
    .expect("expander initialization failed");

    let mut previous = None;
    let mut ticker = Ticker::every(Duration::from_millis(1));
    loop {
        // Read the whole port once; LED pin levels are not button samples.
        let pressed = !port.read_inputs().await.expect("input read failed") & BUTTON_MASK;
        if previous != Some(pressed) {
            let mut leds_on = 0;
            for (button, led) in [
                (BUTTON_CCW, LED_CCW),
                (BUTTON_STOP, LED_STOP),
                (BUTTON_CW, LED_CW),
            ] {
                if pressed & button.mask() != 0 {
                    leds_on |= led.mask();
                }
            }
            // Latches remain zero. Change only LED directions, preserving inputs:
            // configuration bit 0 = sink LOW/on; bit 1 = high impedance/off.
            port.update_configuration(LED_MASK, !leds_on)
                .await
                .expect("LED direction update failed");
            defmt::info!(
                "Buttons: CCW={} STOP={} CW={} MINUS={} PLUS={}",
                pressed & BUTTON_CCW.mask() != 0,
                pressed & BUTTON_STOP.mask() != 0,
                pressed & BUTTON_CW.mask() != 0,
                pressed & BUTTON_MINUS.mask() != 0,
                pressed & BUTTON_PLUS.mask() != 0,
            );
            previous = Some(pressed);
        }
        // Raw panel test: no debounce or machine STOP policy. I/O/logging may
        // overrun this nominal interval; this is not a guaranteed response bound.
        ticker.next().await;
    }
}
