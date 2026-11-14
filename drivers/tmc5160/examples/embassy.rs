//! Diagnostic-only BTT TMC5160T Plus example. EN stays high; no motion.
//! GP18 SCK, GP19 MOSI, GP16 MISO, GP17 CS, GP20 EN, GP21 STEP, GP22 DIR.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{
    bind_interrupts, dma,
    gpio::{Level, Output},
    peripherals, spi,
};
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use panic_probe as _;
use polis_tmc5160::{
    BoardConfig, Timing, Tmc5160,
    registers::{DrvStatus, Gstat, Ioin},
};

bind_interrupts!(struct Irqs {
    DMA_IRQ_0 => dma::InterruptHandler<peripherals::DMA_CH0>, dma::InterruptHandler<peripherals::DMA_CH1>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    // Keep these pins owned for the life of the example. No motor enable or pulses.
    let _enable = Output::new(p.PIN_20, Level::High);
    let _step = Output::new(p.PIN_21, Level::Low);
    let _dir = Output::new(p.PIN_22, Level::Low);
    let cs = Output::new(p.PIN_17, Level::High);
    let mut config = spi::Config::default();
    config.frequency = 1_000_000;
    config.polarity = spi::Polarity::IdleHigh;
    config.phase = spi::Phase::CaptureOnSecondTransition;
    let bus = spi::Spi::new(
        p.SPI0, p.PIN_18, p.PIN_19, p.PIN_16, p.DMA_CH0, p.DMA_CH1, Irqs, config,
    );
    let device = ExclusiveDevice::new(bus, cs, Delay).unwrap();
    let mut driver = Tmc5160::new(
        device,
        Delay,
        BoardConfig::BTT_TMC5160T_PLUS,
        Timing::INTERNAL_CLOCK,
    );
    // Await transfers to completion. This adapter does not restore CS on cancellation.
    let input = driver.read::<Ioin>().await.unwrap().value;
    assert_eq!(input.version(), 0x30);
    assert!(input.external_step_dir() && input.driver_disabled());
    loop {
        let global = driver.read::<Gstat>().await.unwrap().value;
        let status = driver.read::<DrvStatus>().await.unwrap();
        defmt::info!(
            "GSTAT={=u32:x} DRV_STATUS={=u32:x} SPI={=u8:x}",
            global.bits(),
            status.value.bits(),
            status.response_status.bits()
        );
        Timer::after_secs(1).await;
    }
}
