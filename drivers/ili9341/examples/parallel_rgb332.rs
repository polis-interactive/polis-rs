//! ILI9341 8080: GP2..9 D0..7, GP10 WR, GP11 DC, GP12 CS, GP13 RESET.
//! One-byte RGB332 framebuffer; PIO expands to RGB565 without a CPU copy.
//! Hold RD high (3.3 V). Verify shield logic levels and actual LCD pin labels.
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
    peripherals,
    pio::{self, Pio},
};
use embassy_time::{Delay, Timer};
use mousefood::{EmbeddedBackend, embedded_graphics::mono_font::ascii::FONT_10X20};
use panic_probe as _;
use polis_ili9341::{
    Config, Ili9341Rgb332, RGB332_BUFFER_SIZE,
    parallel::{BusConfig, Pins, PioI8080, PioRgb332Interface},
};
use ratatui_core::{
    style::Stylize,
    terminal::Terminal,
    text::{Line, Text},
};

#[global_allocator]
static HEAP: embedded_alloc::LlffHeap = embedded_alloc::LlffHeap::empty();

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<peripherals::PIO0>;
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
    static mut FRAMEBUFFER: [u8; RGB332_BUFFER_SIZE] = [0; RGB332_BUFFER_SIZE];
    // SAFETY: main runs once. The heap owns HEAP_MEMORY for the program's lifetime;
    // FRAMEBUFFER has one exclusive reference and is never accessed elsewhere.
    // Static storage avoids putting a 75 KiB temporary on the stack.
    let framebuffer = unsafe {
        HEAP.init(core::ptr::addr_of_mut!(HEAP_MEMORY) as usize, HEAP_SIZE);
        &mut *core::ptr::addr_of_mut!(FRAMEBUFFER)
    };

    let cs = Output::new(p.PIN_12, Level::High);
    let dc = Output::new(p.PIN_11, Level::High);
    let reset = Output::new(p.PIN_13, Level::High);
    let Pio {
        mut common,
        sm0,
        irq0,
        irq_flags,
        ..
    } = Pio::new(p.PIO0, Irqs);
    let pins = Pins {
        data: [
            common.make_pio_pin(p.PIN_2),
            common.make_pio_pin(p.PIN_3),
            common.make_pio_pin(p.PIN_4),
            common.make_pio_pin(p.PIN_5),
            common.make_pio_pin(p.PIN_6),
            common.make_pio_pin(p.PIN_7),
            common.make_pio_pin(p.PIN_8),
            common.make_pio_pin(p.PIN_9),
        ],
        wr: common.make_pio_pin(p.PIN_10),
        dc,
        cs,
    };
    let bus = PioI8080::new(
        &mut common,
        sm0,
        dma::Channel::new(p.DMA_CH0, Irqs),
        irq0,
        irq_flags,
        pins,
        BusConfig::default(),
    )
    .unwrap();
    let interface = PioRgb332Interface::new(bus, &mut common);
    Timer::after_millis(100).await; // Example supply-settling delay.
    let mut display =
        Ili9341Rgb332::new(interface, reset, framebuffer, Config::default(), &mut Delay)
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
                    Line::from("ILI9341 RGB332 PIO").white(),
                    Line::from("Mousefood + Ratatui").cyan(),
                    Line::from("RED").red(),
                    Line::from("GREEN").green(),
                    Line::from("BLUE").blue(),
                    Line::from(if tick { "Tick" } else { "Tock" }).white(),
                ]);
                frame.render_widget(text, frame.area());
            })
            .unwrap();
        // Terminal::draw only changes local pixels. PIO/DMA work happens here.
        terminal.backend_mut().display_mut().flush().await.unwrap();
        tick = !tick;
        Timer::after_secs(1).await;
    }
}
