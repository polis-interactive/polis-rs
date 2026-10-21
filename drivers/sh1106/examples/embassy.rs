//! Mousefood/Ratatui on a 128×64 SH1106: GP6 SDA, GP7 SCL, address 0x3C.
#![no_std]
#![no_main]

use core::mem::MaybeUninit;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{bind_interrupts, i2c, peripherals};
use embassy_time::Timer;
use mousefood::{EmbeddedBackend, embedded_graphics::mono_font::ascii::FONT_6X10};
use panic_probe as _;
use polis_sh1106::Sh1106Async;
use ratatui_core::{terminal::Terminal, text::Text};

// Ratatui uses allocation; the display driver itself does not.
#[global_allocator]
static HEAP: embedded_alloc::LlffHeap = embedded_alloc::LlffHeap::empty();

bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<peripherals::I2C1>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    const HEAP_SIZE: usize = 32 * 1024;
    static mut HEAP_MEMORY: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
    // SAFETY: main runs once; this static storage is exclusively owned by HEAP
    // for the program's lifetime and is initialized before any allocation.
    unsafe { HEAP.init(core::ptr::addr_of_mut!(HEAP_MEMORY) as usize, HEAP_SIZE) };

    let mut config = i2c::Config::default();
    config.frequency = 400_000;
    config.sda_pullup = false;
    config.scl_pullup = false;
    let bus = i2c::I2c::new_async(p.I2C1, p.PIN_7, p.PIN_6, Irqs, config);
    // Example assumes a module with onboard power-on reset and pull-ups to 3.3 V.
    Timer::after_millis(100).await;
    let mut display = Sh1106Async::new(bus);
    display.init().await.unwrap();

    // 6×10 cells give 21 columns and 6 rows on this small panel.
    let backend = EmbeddedBackend::new(
        &mut display,
        mousefood::EmbeddedBackendConfig {
            font_regular: FONT_6X10,
            ..Default::default()
        },
    );
    let mut terminal = Terminal::new(backend).unwrap();
    let mut tick = false;
    loop {
        terminal
            .draw(|frame| {
                let text = if tick {
                    "SH1106 + Ratatui\nMousefood\nTick"
                } else {
                    "SH1106 + Ratatui\nMousefood\nTock"
                };
                frame.render_widget(Text::from(text), frame.area());
            })
            .unwrap();
        // Ratatui renders into the local framebuffer synchronously. This async
        // call is what actually transfers the pixels over I²C.
        terminal.backend_mut().display_mut().flush().await.unwrap();
        tick = !tick;
        Timer::after_secs(1).await;
    }
}
