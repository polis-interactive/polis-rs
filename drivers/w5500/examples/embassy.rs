//! W5500 raw frame reception: GP18 SCK, GP19 MOSI, GP16 MISO,
//! GP17 CS, GP20 INT, GP21 RESET. See README.md for wiring and scope.
//! SPI setup follows SlateCode/bsp/controller-board; see PROVENANCE.md.
#![no_std]
#![no_main]

use core::{future::poll_fn, task::Poll};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net_driver::{Driver, RxToken};
use embassy_rp::{
    bind_interrupts, dma,
    gpio::{Input, Level, Output, Pull},
    peripherals, spi,
};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use panic_probe as _;
use polis_w5500::{State, W5500};

bind_interrupts!(struct Irqs {
    DMA_IRQ_0 => dma::InterruptHandler<peripherals::DMA_CH0>, dma::InterruptHandler<peripherals::DMA_CH1>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let reset = Output::new(p.PIN_21, Level::Low);
    let cs = Output::new(p.PIN_17, Level::High);
    let interrupt = Input::new(p.PIN_20, Pull::Up);
    let mut config = spi::Config::default();
    config.frequency = 8_000_000;
    let spi = spi::Spi::new(
        p.SPI0, p.PIN_18, p.PIN_19, p.PIN_16, p.DMA_CH0, p.DMA_CH1, Irqs, config,
    );
    let spi = ExclusiveDevice::new(spi, cs, Delay).unwrap();
    // A locally administered unicast MAC: assign a unique value per board.
    let mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
    let mut state = State::<4, 4>::new();
    let (mut device, runner) = W5500::new(spi, interrupt, reset)
        .start(&mut state, mac)
        .await
        .unwrap();
    defmt::info!("W5500 ready; connect Ethernet to receive frames");

    // The runner must stay polled while the device is used. Joining here keeps
    // storage in the main task; separately spawned tasks need static storage.
    // For IP networking, pass device to embassy_net::new instead of this loop,
    // and run both the W5500 runner and the network stack's runner.
    join(runner.run(), async {
        loop {
            let len = poll_fn(|cx| match device.receive(cx) {
                Some((rx, _tx)) => Poll::Ready(rx.consume(|frame| frame.len())),
                None => Poll::Pending,
            })
            .await;
            defmt::info!("Received Ethernet frame: {} bytes", len);
        }
    })
    .await;
}
