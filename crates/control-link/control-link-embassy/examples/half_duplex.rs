//! Two-node request/reply demo, not motor firmware. See README for wiring/timing.
//! GP22 grounded = initiator; GP22 open (pull-up) = responder. Reset both to restart.
#![no_std]
#![no_main]

use core::num::NonZeroU64;
use defmt_rtt as _;
use embassy_rp::{
    bind_interrupts,
    clocks::{ClockConfig, RoscRng},
    gpio::{Input, Level, Output, Pull},
    peripherals, pio,
};
use embassy_time::{Delay, Duration, Instant, Timer, with_timeout};
use embedded_hal::digital::PinState;
use embedded_io_async::{Read, Write};
use panic_probe as _;
use polis_control_link_embassy::{
    Buffers,
    half_duplex::Link,
    protocol::{ControlLink, Incoming, Message, RequestGate, Requests},
};
use polis_pio_uart::{PioUart, Programs};
use polis_rs485::{Config, Rs485};

const CODEC: ControlLink = ControlLink::new(*b"half-duplex-demo");
const HELLO: &[u8] = b"hello-v1";
const IDENTITY: &[u8] = b"half-duplex-demo-v1";
const SEND_BUDGET: Duration = Duration::from_millis(5);
const REPLY_BUDGET: Duration = Duration::from_millis(20);

bind_interrupts!(struct Irqs {
    PIO1_IRQ_0 => pio::InterruptHandler<peripherals::PIO1>;
});

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: embassy_executor::Spawner) {
    let mut config = embassy_rp::config::Config::default();
    config.clocks = ClockConfig::system_freq(128_000_000).unwrap();
    let p = embassy_rp::init(config);
    let role = Input::new(p.PIN_22, Pull::Up);
    let direction = Output::new(p.PIN_19, Level::Low);
    let mut pio = pio::Pio::new(p.PIO1, Irqs);
    let programs = Programs::new(&mut pio.common);
    let uart = PioUart::new(
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
        uart,
        direction,
        Delay,
        Config {
            tx_level: PinState::High,
            // THVD1420 example timings, NOT HC's ST3485/MAX3485 settings.
            enable_us: 12,
            hold_us: 1,
            receive_us: 12,
        },
    );
    with_timeout(SEND_BUDGET, bus.recover())
        .await
        .unwrap()
        .unwrap();
    let (mut receive, mut payload, mut transmit) = ([0; 160], [0; 128], [0; 160]);
    let mut link = Link::new(
        bus,
        CODEC,
        Buffers {
            receive: &mut receive,
            payload: &mut payload,
            transmit: &mut transmit,
        },
    );
    let _ = if role.is_low() {
        defmt::info!("Initiator: waiting one second for responder startup");
        Timer::after_secs(1).await;
        initiator(&mut link).await
    } else {
        defmt::info!("Responder: listening");
        responder(&mut link).await
    };

    // A missing response means unknown outcome. Retire everything; never replay.
    // This demo stops instead of inventing a product reconnect/session policy.
    let mut bus = link.into_inner();
    match with_timeout(SEND_BUDGET, bus.recover()).await {
        Ok(Ok(())) => defmt::warn!("Link ended; TX drained with DE inactive. Reset both boards"),
        _ => defmt::warn!("Link ended; recovery failed. Reset both boards"),
    }
    core::future::pending::<()>().await;
}

async fn initiator<S: Read + Write>(link: &mut Link<'_, S>) -> Result<(), ()> {
    // Fresh demo nonce on each boot. Product nonce/identity policy stays downstream.
    let session = loop {
        if let Some(nonce) = NonZeroU64::new(RoscRng.next_u64()) {
            break nonce;
        }
    };
    let mut requests = Requests::<1>::new(session);
    exchange(link, &mut requests, HELLO, IDENTITY).await?;
    defmt::info!("Identity accepted");
    loop {
        Timer::after_millis(100).await;
        exchange(link, &mut requests, b"ping", b"pong").await?;
        // Logging is outside an active reply window (PIO RX has a small FIFO).
        defmt::info!("Matched pong");
    }
}

async fn exchange<S: Read + Write>(
    link: &mut Link<'_, S>,
    requests: &mut Requests<1>,
    body: &[u8],
    expected: &[u8],
) -> Result<(), ()> {
    let key = requests.reserve().map_err(|_| ())?;
    with_timeout(SEND_BUDGET, link.send(Message::Request { key, body }))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    // One absolute budget for all received messages, not a fresh timeout per frame.
    with_timeout(REPLY_BUDGET, async {
        loop {
            let reply = link
                .receive(|message| match requests.receive(message) {
                    Ok(Incoming::Response { body, .. }) => Some(body == expected),
                    _ => None, // Ignore stale/unmatched frames without extending the window.
                })
                .await
                .map_err(|_| ())?;
            if let Some(matches) = reply {
                return if matches { Ok(()) } else { Err(()) };
            }
        }
    })
    .await
    .map_err(|_| ())?
}

async fn responder<S: Read + Write>(link: &mut Link<'_, S>) -> Result<(), ()> {
    let mut gate = None;
    loop {
        let request = link
            .receive(|message| match message {
                Message::Request { key, body } => Some((key, body == HELLO, body == b"ping")),
                _ => None,
            })
            .await
            .map_err(|_| ())?;
        let Some((key, hello, ping)) = request else {
            continue;
        };
        let received_at = Instant::now();
        let reply: &[u8] = if gate.is_none() && hello {
            // Demo identity exchange only, not authentication or motion authorization.
            gate = Some(RequestGate::new(key.session));
            IDENTITY
        } else if ping {
            b"pong"
        } else {
            b"unsupported"
        };
        let Some(gate) = gate.as_mut() else {
            continue; // Require hello before accepting requests.
        };
        if gate.accept(key).is_err() {
            continue; // No work or reply for stale/duplicate requests.
        }
        // Allow the initiator's physical TX tail/DE release/RX settling to finish.
        Timer::after_millis(1).await;
        if Instant::now() - received_at >= Duration::from_millis(5) {
            return Err(()); // Never start a late reply after an executor stall.
        }
        // At most 5 ms before starting + 5 ms to send, inside the 20 ms window.
        with_timeout(
            SEND_BUDGET,
            link.send(Message::Response { key, body: reply }),
        )
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    }
}
