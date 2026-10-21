//! ILI9341 SPI: GP18 SCK, GP19 MOSI, GP17 CS, GP20 DC, GP21 RESET.
//! See README.md for module power, LED and unused touch/SD connections.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use core::mem::MaybeUninit;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{
    bind_interrupts, dma,
    gpio::{Level, Output},
    peripherals, spi,
};
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use mousefood::{EmbeddedBackend, embedded_graphics::mono_font::ascii::FONT_10X20};
use panic_probe as _;
use polis_ili9341::{BUFFER_SIZE, Config, Ili9341Async, SpiInterface};
use ratatui_core::{
    style::Stylize,
    terminal::Terminal,
    text::{Line, Text},
};

#[global_allocator]
static HEAP: embedded_alloc::LlffHeap = embedded_alloc::LlffHeap::empty();

bind_interrupts!(struct Irqs {
    DMA_IRQ_0 => dma::InterruptHandler<peripherals::DMA_CH0>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    const HEAP_SIZE: usize = 64 * 1024;
    static mut HEAP_MEMORY: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
    static mut FRAMEBUFFER: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
    // SAFETY: main runs once. The heap owns HEAP_MEMORY for the program's lifetime;
    // FRAMEBUFFER has one exclusive reference and is never accessed elsewhere.
    // Static storage avoids putting a 150 KiB temporary on the stack.
    let framebuffer = unsafe {
        HEAP.init(core::ptr::addr_of_mut!(HEAP_MEMORY) as usize, HEAP_SIZE);
        &mut *core::ptr::addr_of_mut!(FRAMEBUFFER)
    };

    let cs = Output::new(p.PIN_17, Level::High);
    let dc = Output::new(p.PIN_20, Level::High);
    let reset = Output::new(p.PIN_21, Level::High);
    let mut config = spi::Config::default();
    config.frequency = 8_000_000; // Mode 0; start conservatively for jumper wiring.
    let bus = spi::Spi::new_txonly(p.SPI0, p.PIN_18, p.PIN_19, p.DMA_CH0, Irqs, config);
    let device = ExclusiveDevice::new(bus, cs, Delay).unwrap();
    let interface = SpiInterface::new(device, dc);
    Timer::after_millis(100).await; // Example supply-settling delay.
    let mut display =
        Ili9341Async::new(interface, reset, framebuffer, Config::default(), &mut Delay)
            .await
            .unwrap();

    // 32 columns × 12 rows limits Ratatui's cell-buffer allocation on Pico.
    let backend = EmbeddedBackend::new(
        &mut display,
        mousefood::EmbeddedBackendConfig {
            font_regular: FONT_10X20,
            ..Default::default()
        },
    );
    let mut terminal = Terminal::new(backend).unwrap();
    let mut tick = false;
    loop {
        terminal
            .draw(|frame| {
                let text = Text::from(vec![
                    Line::from("ILI9341 320x240").white(),
                    Line::from("Mousefood + Ratatui").cyan(),
                    Line::from("RED").red(),
                    Line::from("GREEN").green(),
                    Line::from("BLUE").blue(),
                    Line::from(if tick { "Tick" } else { "Tock" }).white(),
                ]);
                frame.render_widget(text, frame.area());
            })
            .unwrap();
        // Terminal::draw only changes local pixels. SPI/DMA work happens here.
        terminal.backend_mut().display_mut().flush().await.unwrap();
        tick = !tick;
        Timer::after_secs(1).await;
    }
}
