//! One task owns three 128×32 SSD1306s at 0x3C on PCA9548A channels 0, 1 and 2.
//! Upstream: GP6 SDA, GP7 SCL; mux 0x70. See README for wiring and build commands.
#![no_std]
#![no_main]

mod support;

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_time::{Duration, Ticker, Timer};
use panic_probe as _;
use polis_ssd1306::Ssd1306Async;
use polis_xca954xa::Channel;

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let mux = support::init_mux();
    // Modules must provide power-on reset; this is their startup settling delay.
    Timer::after_millis(100).await;
    let mut screens = [
        Ssd1306Async::new(mux.channel(Channel::Ch0)),
        Ssd1306Async::new(mux.channel(Channel::Ch1)),
        Ssd1306Async::new(mux.channel(Channel::Ch2)),
    ];
    for screen in &mut screens {
        screen.init().await.unwrap();
    }

    let labels = ["SCREEN 1 / CH0", "SCREEN 2 / CH1", "SCREEN 3 / CH2"];
    let mut ticker = Ticker::every(Duration::from_millis(100));
    let mut frame = 0_u32;
    loop {
        for (screen, label) in screens.iter_mut().zip(labels) {
            support::draw_frame(screen, label, frame);
            screen.flush().await.unwrap();
        }
        frame = frame.wrapping_add(1);
        ticker.next().await;
    }
}
