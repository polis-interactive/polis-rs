//! Three independent encoders on Pico/Pico 2. Contacts connect to ground.
//! (B, A) = (GP1, GP2), (GP4, GP5), (GP7, GP8). Buttons are not part of this driver.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{bind_interrupts, peripherals, pio};
use embassy_time::Timer;
use panic_probe as _;
use polis_pio_encoder::{
    Config, Direction, PioEncoder, PioEncoderProgram, Position, Resolution, Tracker,
};

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<peripherals::PIO0>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let mut pio = pio::Pio::new(p.PIO0, Irqs);
    let program = PioEncoderProgram::new(&mut pio.common).unwrap();
    // Integer divider 125: ~1 MHz PIO at Pico's default clk_sys. Pico 2's
    // default is different; recompute the bound if the application changes clocks.
    let config = Config {
        clock_divider: 125u16.into(),
        ..Default::default()
    };
    let mut e0 = PioEncoder::new(&mut pio.common, pio.sm0, p.PIN_1, p.PIN_2, &program, config);
    let mut e1 = PioEncoder::new(&mut pio.common, pio.sm1, p.PIN_4, p.PIN_5, &program, config);
    let mut e2 = PioEncoder::new(&mut pio.common, pio.sm2, p.PIN_7, p.PIN_8, &program, config);
    let mut trackers = core::array::from_fn::<_, 3, _>(|_| {
        Tracker::new(Position(0), Resolution::FullStep, Direction::ALeadsB)
    });
    loop {
        let samples = [
            e0.snapshot().await,
            e1.snapshot().await,
            e2.snapshot().await,
        ];
        let mut positions = [0i64; 3];
        for (i, sample) in samples.into_iter().enumerate() {
            positions[i] = trackers[i].update(sample).unwrap().position;
        }
        defmt::info!("Net full-step positions: {:?}", positions);
        // PIO keeps counting throughout this delay, including when RX is full.
        // A real application retains trackers independently of its transport.
        Timer::after_millis(100).await;
    }
}
