//! Discovery controller link fixture: GP18 DI, GP20 RO, GP19 tied DE + /RE.
//! Reuses this repository's RS-485 example setup; see PROVENANCE.md.
//! Validates linking, not the receive buffering or timing of a physical bus.
#![no_std]
#![no_main]

use bus_driver::{Config, Rs485};
use core::num::{NonZeroU16, NonZeroU32};
use defmt_rtt as _;
use discovery_embassy::rs485::{DiscoveryRound, discover};
use discovery_protocol::{ApplicationId, Discovery};
use embassy_executor::Spawner;
use embassy_rp::{
    bind_interrupts,
    clocks::ClockConfig,
    gpio::{Level, Output},
    peripherals, pio,
};
use embassy_time::{Delay, Timer};
use embedded_hal::digital::PinState;
use panic_probe as _;
use pio_uart::{PioUart, Programs};

bind_interrupts!(struct Irqs {
    PIO1_IRQ_0 => pio::InterruptHandler<peripherals::PIO1>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let mut config = embassy_rp::config::Config::default();
    config.clocks = ClockConfig::system_freq(128_000_000).unwrap();
    let p = embassy_rp::init(config);
    let direction = Output::new(p.PIN_19, Level::Low);
    let mut pio = pio::Pio::new(p.PIO1, Irqs);
    let programs = Programs::new(&mut pio.common);
    // This wrapper's flush waits for wire completion before DE is released.
    let serial = PioUart::new(
        &mut pio.common,
        pio.sm0,
        pio.sm1,
        p.PIN_18,
        p.PIN_20,
        &programs,
        1_000_000,
    )
    .unwrap();
    let mut bus = Rs485::new(
        serial,
        direction,
        Delay,
        Config {
            tx_level: PinState::High,
            enable_us: 12,
            hold_us: 1,
            receive_us: 12,
        },
    );
    bus.recover().await.unwrap(); // Drain stale TX bytes with DE inactive.
    let discovery = Discovery::new(ApplicationId(*b"example-app-0001"));
    let mut seed = 0u32;
    loop {
        seed = seed.wrapping_add(1);
        let round = DiscoveryRound {
            discovery_id: 1, // Fixture only: deployments generate a fresh session ID.
            seed,
            slot_count: NonZeroU16::new(16).unwrap(),
            slot_duration_us: NonZeroU32::new(10_000).unwrap(),
            turnaround_us: 1000,
        };
        discover::<16, _>(
            &mut bus,
            &discovery,
            round,
            embassy_time::Duration::from_secs(1),
            |_| {},
        )
        .await
        .unwrap();
        Timer::after_secs(1).await;
    }
}
