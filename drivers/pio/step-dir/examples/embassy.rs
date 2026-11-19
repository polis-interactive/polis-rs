//! Logic-analyzer fixture. Motor power disconnected; EN_N remains high.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_futures::select::select;
use embassy_rp::{
    bind_interrupts,
    clocks::ClockConfig,
    dma,
    gpio::{Level, Output},
    peripherals,
    pio::{self, Pio},
};
use embassy_time::Timer;
use panic_probe as _;
use polis_pio_step_dir::{Config, Direction, Error, Pins, PioStepDir, Segment, StopReason};

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<peripherals::PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<peripherals::DMA_CH0>;
});

type Engine = PioStepDir<'static, peripherals::PIO0, 0>;

async fn submit(engine: &mut Engine, segment: Segment) {
    loop {
        match engine.commit(&[segment]).await {
            Ok(()) => return,
            Err(Error::QueueFull | Error::Horizon) => Timer::after_micros(100).await,
            Err(error) => panic!("pulse execution failed: {:?}", error),
        }
    }
}

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let mut config = embassy_rp::config::Config::default();
    config.clocks = ClockConfig::system_freq(128_000_000).unwrap();
    let p = embassy_rp::init(config);
    let _disabled = Output::new(p.PIN_22, Level::High);
    let Pio {
        mut common,
        sm0,
        irq0,
        irq_flags,
        ..
    } = Pio::new(p.PIO0, Irqs);
    let pins = Pins {
        step: common.make_pio_pin(p.PIN_20),
        dir: common.make_pio_pin(p.PIN_21),
    };
    let mut engine = PioStepDir::new(
        &mut common,
        sm0,
        dma::Channel::new(p.DMA_CH0, Irqs),
        irq0,
        irq_flags,
        pins,
        Config {
            tick_hz: 4_000_000,
            step_high_ticks: 4,
            dir_hold_ticks: 4,
            dir_setup_ticks: 4,
            max_committed_ticks: 8_000,
        },
    )
    .unwrap();

    loop {
        for direction in [Direction::Positive, Direction::Negative] {
            let cruise = Segment::Pulses {
                direction,
                pulses: 40,
                period_ticks: 80,
            };
            engine.commit(&[cruise, cruise]).await.unwrap();
            engine.start().unwrap();
            // Electrical timing exercise, NOT a calibrated mechanical ramp.
            // 50 kHz down to 5 kHz, with admission bounded to 2 ms of work.
            for period_ticks in (80..=800).step_by(40) {
                submit(
                    &mut engine,
                    Segment::Pulses {
                        direction,
                        pulses: (3200 / period_ticks) as u16,
                        period_ticks,
                    },
                )
                .await;
            }
            submit(&mut engine, Segment::End).await;
            let report = engine.wait_stopped().await.unwrap();
            defmt::info!(
                "completed edges={} signed={}",
                report.emitted_pulses,
                report.emitted_signed_pulses
            );
            Timer::after_millis(100).await;
            engine.reset().unwrap();
        }
        engine
            .commit(&[Segment::Pulses {
                direction: Direction::Positive,
                pulses: 80,
                period_ticks: 80,
            }])
            .await
            .unwrap();
        engine.start().unwrap();
        Timer::after_micros(500).await;
        let report = engine.abort();
        defmt::info!(
            "abort edges={} possibly truncated={}",
            report.emitted_pulses,
            report.pulse_may_be_truncated
        );
        // Observe STEP remaining low until the explicit reset and next start.
        Timer::after_millis(100).await;
        engine.reset().unwrap();
        engine
            .commit(&[Segment::Pulses {
                direction: Direction::Negative,
                pulses: 80,
                period_ticks: 80,
            }])
            .await
            .unwrap();
        engine.start().unwrap();
        // Cancelling a pending wait must leave no stale DMA/FIFO work able to run.
        let _ = select(engine.wait_stopped(), Timer::after_micros(501)).await;
        let report = engine.poll().unwrap_err();
        assert_eq!(report.reason, StopReason::Cancelled);
        defmt::info!(
            "cancel edges={} possibly truncated={}",
            report.emitted_pulses,
            report.pulse_may_be_truncated
        );
        Timer::after_millis(100).await;
        engine.reset().unwrap();
    }
}
