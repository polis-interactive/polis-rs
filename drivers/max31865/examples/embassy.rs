//! Three-wire PT100: GP18 SCK, GP19 MOSI, GP16 MISO, GP17 CS.
//! Set board jumpers and reference calibration to match the actual RTD.
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
use polis_max31865::{Config, Error, Max31865};

bind_interrupts!(struct Irqs {
    DMA_IRQ_0 => dma::InterruptHandler<peripherals::DMA_CH0>, dma::InterruptHandler<peripherals::DMA_CH1>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let cs = Output::new(p.PIN_17, Level::High);
    let mut config = spi::Config::default();
    config.frequency = 1_000_000;
    config.phase = spi::Phase::CaptureOnSecondTransition;
    let bus = spi::Spi::new(
        p.SPI0, p.PIN_18, p.PIN_19, p.PIN_16, p.DMA_CH0, p.DMA_CH1, Irqs, config,
    );
    let device = ExclusiveDevice::new(bus, cs, Delay).unwrap();
    let mut sensor = Max31865::new(device, Config::pt100_3wire()).unwrap();
    sensor.init().await.unwrap();

    loop {
        match sensor.read_temperature(&mut Delay).await {
            Ok(celsius) => defmt::info!("Temperature: {} C", celsius),
            Err(Error::Fault(status)) => defmt::warn!("RTD fault: 0x{:02x}", status.bits()),
            Err(_) => panic!("MAX31865 measurement failed"),
        }
        Timer::after_secs(1).await;
    }
}
