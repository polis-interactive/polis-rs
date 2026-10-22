//! Flying sprites on the HiLetgo 8080 shield; same wiring as parallel_rgb332.
//! Host preview: rustc --edition=2024 -O drivers/ili9341/examples/parallel_rockets.rs
//!   -o .target/rocket-preview && .target/rocket-preview /tmp/polis-rockets
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

// QUICK SETTINGS — speeds are pixels/second, angles clockwise from straight up.
const SCREEN_WIDTH: usize = 240;
const SCREEN_HEIGHT: usize = 320;
const SPRITE_SCALE: usize = 2; // Native sprite is 11×21; default is 22×42 pixels.
// Aim for 18% bounding-box coverage: 240*320*0.18 / (22*42) ≈ 14 rockets.
const N: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 18 / (100 * 11 * 21 * SPRITE_SCALE * SPRITE_SCALE);
const MIN_SPEED: f32 = 28.0;
const MAX_SPEED: f32 = 70.0;
const HEADINGS: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7]; // Multiples of 45°; edit this subset.
const TARGET_FPS: u64 = 30; // Ceiling; actual rate is limited by LCD transfers.
const BUS_MAX_CLOCK_HZ: u32 = 50_000_000; // PIO clock request, not WR frequency. Try 24 or 12 MHz if pixels corrupt.
const MAX_ROCKETS: usize = N * 4 + 8; // Includes rockets approaching/leaving the screen.
const STAR_COUNT: usize = 80;
const BACKGROUND_RGB332: u8 = 0x25;
const OFFSCREEN_MARGIN: f32 = 10.0 * SPRITE_SCALE as f32; // Exhaust clearance; at least 7 * scale.
const WARMUP_SECONDS: u64 = 30; // Simulate offscreen launches before showing the first frame; 0 = empty start.
const RANDOM_SEED: u32 = 0x726f_636b; // Fixed seed makes tuning reproducible.
const SHOW_HUD: bool = true;
const STAGING_ROWS: usize = 8;

#[path = "rocket_support/scene.rs"]
mod scene;

const SETTINGS: scene::Settings = scene::Settings {
    width: SCREEN_WIDTH,
    height: SCREEN_HEIGHT,
    scale: SPRITE_SCALE,
    target: N,
    min_speed: MIN_SPEED,
    max_speed: MAX_SPEED,
    headings: HEADINGS,
    background: BACKGROUND_RGB332,
    margin: OFFSCREEN_MARGIN,
};
type Scene = scene::Scene<MAX_ROCKETS, STAR_COUNT>;

#[cfg(not(target_os = "none"))]
#[path = "rocket_support/preview.rs"]
mod preview;

#[cfg(not(target_os = "none"))]
fn main() {
    preview::run();
}

#[cfg(target_os = "none")]
mod firmware {
    use super::*;
    use core::fmt::Write;
    use defmt_rtt as _;
    use embassy_executor::Spawner;
    use embassy_rp::{
        bind_interrupts, dma,
        gpio::{Level, Output},
        peripherals,
        pio::{self, Pio},
    };
    use embassy_time::{Delay, Duration, Instant, Timer};
    use embedded_graphics::{
        mono_font::{MonoTextStyle, ascii::FONT_6X10},
        pixelcolor::Rgb565,
        prelude::*,
        primitives::{PrimitiveStyle, Rectangle},
        text::{Baseline, Text},
    };
    use panic_probe as _;
    use polis_ili9341::{
        Config, Ili9341Rgb332, RGB332_BUFFER_SIZE, Rotation,
        parallel::{
            BusConfig, Pins, PioI8080, PioI8080Rgb332Pipeline, PioRgb332PipelineInterface,
            Rgb332Converter,
        },
    };

    bind_interrupts!(struct Irqs {
        PIO0_IRQ_0 => pio::InterruptHandler<peripherals::PIO0>;
        DMA_IRQ_0 => dma::InterruptHandler<peripherals::DMA_CH0>,
                     dma::InterruptHandler<peripherals::DMA_CH1>,
                     dma::InterruptHandler<peripherals::DMA_CH2>;
    });

    struct Label {
        bytes: [u8; 64],
        len: usize,
    }
    impl Write for Label {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let end = self.len + s.len();
            let dst = self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?;
            dst.copy_from_slice(s.as_bytes());
            self.len = end;
            Ok(())
        }
    }

    #[embassy_executor::main(
        executor = "embassy_rp::executor::Executor",
        entry = "cortex_m_rt::entry"
    )]
    async fn main(_spawner: Spawner) {
        let p = embassy_rp::init(Default::default());
        let mut led = Output::new(p.PIN_25, Level::Low);
        for _ in 0..3 {
            led.set_high();
            Timer::after_millis(150).await;
            led.set_low();
            Timer::after_millis(150).await;
        }
        // All large storage is static: no heap, no full-frame stack temporaries.
        static mut FRAME: [u8; RGB332_BUFFER_SIZE] = [0; RGB332_BUFFER_SIZE];
        static mut STAGING: [u16; 2 * SCREEN_WIDTH * STAGING_ROWS] =
            [0; 2 * SCREEN_WIDTH * STAGING_ROWS];
        static mut SCENE: Scene = Scene::empty();
        // SAFETY: this entry runs once and retains the only references.
        let (frame, staging, scene) = unsafe {
            (
                &mut *core::ptr::addr_of_mut!(FRAME),
                &mut *core::ptr::addr_of_mut!(STAGING),
                &mut *core::ptr::addr_of_mut!(SCENE),
            )
        };
        scene.init(SETTINGS, RANDOM_SEED);
        for _ in 0..WARMUP_SECONDS * 20 {
            scene.advance(50_000);
        }
        scene.render(frame);

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
        let bus = PioI8080::new(
            &mut common,
            sm0,
            dma::Channel::new(p.DMA_CH0, Irqs),
            irq0,
            irq_flags,
            pins,
            BusConfig {
                max_clock_hz: BUS_MAX_CLOCK_HZ,
            },
        )
        .unwrap();
        let system_hz = embassy_rp::clocks::clk_sys_freq();
        let divider = BusConfig {
            max_clock_hz: BUS_MAX_CLOCK_HZ,
        }
        .divider(system_hz)
        .unwrap();
        defmt::info!(
            "rocket bus: system_hz={} divider={} pio_hz={}",
            system_hz,
            divider,
            system_hz / u32::from(divider)
        );
        let converter = Rgb332Converter::new(
            &mut common,
            sm1,
            dma::Channel::new(p.DMA_CH1, Irqs),
            dma::Channel::new(p.DMA_CH2, Irqs),
        );
        let interface =
            PioRgb332PipelineInterface::new(PioI8080Rgb332Pipeline::new(bus, converter, staging));
        let mut display = Ili9341Rgb332::new(
            interface,
            reset,
            frame,
            Config {
                mirror_horizontal: true,
                rotation: Rotation::Deg270,
                ..Default::default()
            },
            &mut Delay,
        )
        .await
        .unwrap();
        assert_eq!(
            display.size(),
            Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32)
        );
        assert!(TARGET_FPS > 0 && TARGET_FPS <= 1000);
        let period = Duration::from_micros(1_000_000 / TARGET_FPS);
        let mut previous = Instant::now();
        let mut sample_start = previous;
        let mut frames = 0u32;
        let mut fps_tenths = 0u64;
        // Per-frame means from the previous sampling interval. R includes HUD;
        // L includes conversion, DMA scheduling and waiting for the final write.
        // Frame pacing is excluded from these phases but included in FPS.
        let mut totals_us = [0u64; 3];
        let mut means_us = [0u64; 3];
        loop {
            let start = Instant::now();
            scene.advance(start.duration_since(previous).as_micros());
            previous = start;
            let update_end = Instant::now();
            scene.render(display.buffer_mut());
            if SHOW_HUD {
                let mut label = Label {
                    bytes: [0; 64],
                    len: 0,
                };
                write!(
                    &mut label,
                    "ROCKETS {}/{}  {}.{} FPS\nU {}.{} R {}.{} L {}.{} ms",
                    scene.visible(),
                    N,
                    fps_tenths / 10,
                    fps_tenths % 10,
                    means_us[0] / 1000,
                    means_us[0] / 100 % 10,
                    means_us[1] / 1000,
                    means_us[1] / 100 % 10,
                    means_us[2] / 1000,
                    means_us[2] / 100 % 10
                )
                .unwrap();
                Rectangle::new(Point::zero(), Size::new(SCREEN_WIDTH as u32, 24))
                    .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
                    .draw(&mut display)
                    .unwrap();
                Text::with_baseline(
                    core::str::from_utf8(&label.bytes[..label.len]).unwrap(),
                    Point::new(2, 1),
                    MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE),
                    Baseline::Top,
                )
                .draw(&mut display)
                .unwrap();
            }
            let render_end = Instant::now();
            display.flush().await.unwrap();
            let flush_end = Instant::now();
            totals_us[0] += update_end.duration_since(start).as_micros();
            totals_us[1] += render_end.duration_since(update_end).as_micros();
            totals_us[2] += flush_end.duration_since(render_end).as_micros();
            frames += 1;
            let elapsed = flush_end.duration_since(sample_start).as_micros();
            if elapsed >= 1_000_000 {
                fps_tenths = u64::from(frames) * 10_000_000 / elapsed;
                for i in 0..3 {
                    means_us[i] = totals_us[i] / u64::from(frames);
                }
                defmt::info!(
                    "rockets visible={} target={} fps_x10={} update_us={} render_us={} lcd_us={} capacity_drops={}",
                    scene.visible(),
                    N,
                    fps_tenths,
                    means_us[0],
                    means_us[1],
                    means_us[2],
                    scene.dropped
                );
                frames = 0;
                totals_us = [0; 3];
                sample_start = flush_end;
                led.toggle();
            }
            // No extra delay when rendering/transmission already used the budget.
            Timer::at(start + period).await;
        }
    }
}
