//! Three Embassy tasks each own one SSD1306, sharing one PCA9548A and I²C bus.
//! Same wiring as `embassy`. Tasks are cooperative, not OS threads or CPU cores.
#![no_std]
#![no_main]

mod support;

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Duration, Ticker, Timer};
use panic_probe as _;
use polis_ssd1306::Ssd1306Async;
use polis_xca954xa::{Channel, SharedChannel};
use static_cell::StaticCell;

type Display = Ssd1306Async<SharedChannel<'static, CriticalSectionRawMutex, support::Bus>>;
static MUX: StaticCell<support::Mux> = StaticCell::new();

#[embassy_executor::task(pool_size = 3)]
async fn screen(mut display: Display, label: &'static str, period_ms: u64) {
    display.init().await.unwrap();
    let mut ticker = Ticker::every(Duration::from_millis(period_ms));
    let mut frame = 0_u32;
    loop {
        support::draw_frame(&mut display, label, frame);
        display.flush().await.unwrap();
        frame = frame.wrapping_add(1);
        // No bus lock is held while waiting or drawing the next frame.
        ticker.next().await;
    }
}

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(spawner: Spawner) {
    let mux: &'static support::Mux = MUX.init(support::init_mux());
    Timer::after_millis(100).await;

    // Each task owns its display object, framebuffer and independent schedule.
    for (channel, label, period_ms) in [
        (Channel::Ch0, "TASK 1 / CH0", 100),
        (Channel::Ch1, "TASK 2 / CH1", 150),
        (Channel::Ch2, "TASK 3 / CH2", 200),
    ] {
        let display = Ssd1306Async::new(mux.channel(channel));
        spawner.spawn(screen(display, label, period_ms).unwrap());
    }
}
