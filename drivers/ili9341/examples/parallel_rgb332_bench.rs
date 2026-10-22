//! Compare one-SM expansion with the two-SM pipeline on the same 8-bit panel.
//! GP0..7 = D0..7, GP8 = WR, GP9 = DC, GP10 = CS, GP11 = RESET; RD high.
//! RTT logs report upload latency, not panel refresh rate. No heap or rasterizer.
#![no_std]
#![no_main]

use core::convert::Infallible;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{
    bind_interrupts, dma,
    gpio::{Level, Output},
    peripherals,
    pio::{self, Pio},
};
use embassy_time::{Delay, Duration, Instant, Timer, with_timeout};
use embedded_hal::digital::OutputPin;
use panic_probe as _;
use polis_ili9341::{
    Config, HEIGHT, Ili9341Rgb332, RGB332_BUFFER_SIZE, Rgb332Interface, WIDTH,
    parallel::{
        BusConfig, Pins, PioI8080, PioI8080Rgb332Pipeline, PioRgb332Interface,
        PioRgb332PipelineInterface, Rgb332Converter,
    },
};

// Instruction-clock limit, not WR frequency. Start at 12 MHz if the physical
// connection is unqualified; compare both paths at the same setting.
const BUS_MAX_CLOCK_HZ: u32 = 50_000_000;
const STAGING_ROWS: usize = 8;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<peripherals::PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<peripherals::DMA_CH0>,
                 dma::InterruptHandler<peripherals::DMA_CH1>,
                 dma::InterruptHandler<peripherals::DMA_CH2>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    static mut FRAME: [u8; RGB332_BUFFER_SIZE] = [0; RGB332_BUFFER_SIZE];
    static mut SCRATCH: [u16; 2 * WIDTH * STAGING_ROWS] = [0; 2 * WIDTH * STAGING_ROWS];
    // SAFETY: main runs once; these exclusive references are the only accesses
    // to the static storage. Large buffers never occupy the stack.
    let (frame, scratch) = unsafe {
        (
            &mut *core::ptr::addr_of_mut!(FRAME),
            &mut *core::ptr::addr_of_mut!(SCRATCH),
        )
    };
    // All 256 colors in a 16×16 grid, prepared outside the measured region.
    for (i, pixel) in frame.iter_mut().enumerate() {
        *pixel = (((i / WIDTH) / 15) * 16 + (i % WIDTH) / 20) as u8;
    }
    // This shield needs left-to-right reflection in landscape mode.
    let display_config = Config {
        mirror_horizontal: true,
        ..Default::default()
    };
    let cs = Output::new(p.PIN_10, Level::High);
    let dc = Output::new(p.PIN_9, Level::High);
    let reset = Output::new(p.PIN_11, Level::High);
    let Pio {
        mut common,
        sm0,
        sm1,
        irq0,
        irq_flags,
        ..
    } = Pio::new(p.PIO0, Irqs);
    let pins = Pins {
        data: [
            common.make_pio_pin(p.PIN_0),
            common.make_pio_pin(p.PIN_1),
            common.make_pio_pin(p.PIN_2),
            common.make_pio_pin(p.PIN_3),
            common.make_pio_pin(p.PIN_4),
            common.make_pio_pin(p.PIN_5),
            common.make_pio_pin(p.PIN_6),
            common.make_pio_pin(p.PIN_7),
        ],
        wr: common.make_pio_pin(p.PIN_8),
        dc,
        cs,
    };
    let timing = BusConfig {
        max_clock_hz: BUS_MAX_CLOCK_HZ,
    };
    let system_hz = embassy_rp::clocks::clk_sys_freq();
    defmt::info!(
        "system={} Hz; LCD SM={} Hz; converter={} Hz; scratch={} bytes",
        system_hz,
        system_hz / u32::from(timing.divider(system_hz).unwrap()),
        system_hz,
        core::mem::size_of_val(scratch)
    );
    let bus = PioI8080::new(
        &mut common,
        sm0,
        dma::Channel::new(p.DMA_CH0, Irqs),
        irq0,
        irq_flags,
        pins,
        timing,
    )
    .unwrap();
    Timer::after_millis(100).await;
    let mut display = Ili9341Rgb332::new(
        PioRgb332Interface::new(bus, &mut common),
        reset,
        frame,
        display_config,
        &mut Delay,
    )
    .await
    .unwrap();
    suite("one SM", &mut display).await;

    let (interface, reset, frame) = display.release();
    // Free the 24-slot expansion program before allocating the converter in
    // the same block. The raw output program keeps its seven slots.
    let bus = interface.release().release(&mut common);
    let converter = Rgb332Converter::new(
        &mut common,
        sm1,
        dma::Channel::new(p.DMA_CH1, Irqs),
        dma::Channel::new(p.DMA_CH2, Irqs),
    );
    let interface =
        PioRgb332PipelineInterface::new(PioI8080Rgb332Pipeline::new(bus, converter, scratch));
    let mut display = Ili9341Rgb332::new(interface, reset, frame, display_config, &mut Delay)
        .await
        .unwrap();

    // Exercise cancellation during prefill and overlapped output, followed by
    // complete retries. Inspect CS/WR on an analyzer to qualify cleanup.
    for us in [100, 500, 5000] {
        assert!(
            with_timeout(Duration::from_micros(us), display.flush_rows(0..HEIGHT))
                .await
                .is_err()
        );
        display.flush_rows(0..HEIGHT).await.unwrap();
    }
    defmt::info!("pipeline cancellation/retry futures completed; inspect LCD/analyzer");
    loop {
        suite("two SMs", &mut display).await;
        Timer::after_secs(2).await;
    }
}

async fn suite<DI: Rgb332Interface<Error = Infallible>, RST: OutputPin>(
    label: &str,
    display: &mut Ili9341Rgb332<'_, DI, RST>,
) {
    // Warm up before measurement; setup, rasterization and RTT logging excluded.
    display.flush_rows(0..HEIGHT).await.unwrap();
    for rows in [HEIGHT, 16] {
        let samples: u64 = if rows == HEIGHT { 30 } else { 150 };
        let (mut sum, mut min, mut max) = (0u64, u64::MAX, 0u64);
        for n in 0..samples {
            let y = (n as usize % (HEIGHT / rows)) * rows;
            let start = Instant::now();
            display.flush_rows(y..y + rows).await.unwrap();
            let us = start.elapsed().as_micros();
            sum += us;
            min = min.min(us);
            max = max.max(us);
        }
        defmt::info!(
            "{}: 320x{}, samples={}, us avg/min/max={}/{}/{}, uploads/s x1000={}",
            label,
            rows,
            samples,
            sum / samples,
            min,
            max,
            samples * 1_000_000_000 / sum.max(1)
        );
    }
}
