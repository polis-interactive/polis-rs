#![cfg(feature = "rs485")]
use discovery_protocol::{
    Announcement, ApplicationId, DeviceId, Discovery, Message, rs485::DiscoveryAck,
};
use discovery_tokio::{Error, WIRE_CAPACITY, rs485::*};
use std::{
    io,
    num::{NonZeroU16, NonZeroU32},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    time::Instant,
};
const CODEC: Discovery = Discovery::new(ApplicationId(*b"example-app-0001"));
const DEVICE: DeviceId = DeviceId(*b"device01");
fn round() -> DiscoveryRound {
    DiscoveryRound {
        discovery_id: 7,
        seed: 1,
        slot_count: NonZeroU16::new(4).unwrap(),
        slot_duration_us: NonZeroU32::new(10_000).unwrap(),
        turnaround_us: 1000,
    }
}
fn timing() -> Timing {
    Timing {
        receive_margin_us: 100,
        transmit_budget_us: NonZeroU32::new(500).unwrap(),
    }
}
fn wire(message: Message) -> Vec<u8> {
    let mut payload = [0; discovery_protocol::MAX_PAYLOAD_LEN as usize];
    let mut output = [0; WIRE_CAPACITY];
    framing::encode(
        framing::Header {
            protocol_id: discovery_protocol::PROTOCOL_ID,
            schema_version: 1,
            message_type: message.message_type(),
        },
        CODEC.encode_payload(&message, &mut payload).unwrap(),
        &mut output,
    )
    .unwrap()
    .to_vec()
}
#[derive(Default)]
struct Bus {
    incoming: Vec<u8>,
    cursor: usize,
    sent: Vec<(Instant, Message)>,
    stall: bool,
}
impl AsyncRead for Bus {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.cursor == self.incoming.len() {
            return Poll::Pending;
        }
        let n = 3
            .min(buf.remaining())
            .min(self.incoming.len() - self.cursor);
        buf.put_slice(&self.incoming[self.cursor..self.cursor + n]);
        self.cursor += n;
        Poll::Ready(Ok(()))
    }
}
impl AsyncWrite for Bus {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.stall {
            return Poll::Pending;
        }
        let mut bytes_copy = bytes.to_vec();
        let frame = framing::decode_datagram(&mut bytes_copy, WIRE_CAPACITY).unwrap();
        let message = CODEC
            .decode_payload(
                frame.header.protocol_id,
                frame.header.schema_version,
                frame.header.message_type,
                frame.payload,
            )
            .unwrap();
        self.sent.push((Instant::now(), message));
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test(start_paused = true)]
async fn controller_waits_entire_window_then_acks_only_distinct_retained_replies() {
    let reply = Announcement {
        discovery_id: 7,
        device_id: DEVICE,
    };
    let mut incoming = vec![0, 1, 2, 0]; // collision noise before valid frames
    for message in [
        Announcement {
            discovery_id: 6,
            ..reply
        },
        reply,
        reply,
        Announcement {
            device_id: DeviceId([2; 8]),
            ..reply
        },
    ] {
        incoming.extend(wire(Message::Announcement(message)));
    }
    let mut bus = Bus {
        incoming,
        ..Default::default()
    };
    let mut found = Vec::new();
    let result = discover::<1, _>(&mut bus, &CODEC, round(), Duration::from_secs(1), |a| {
        found.push(a)
    })
    .await
    .unwrap();
    assert_eq!(found, [reply]);
    assert_eq!(
        result,
        ScanSummary {
            found: 1,
            overflow: true
        }
    );
    assert_eq!(bus.sent.len(), 2);
    assert_eq!(
        bus.sent[1].1,
        Message::Rs485DiscoveryAck(DiscoveryAck {
            discovery_id: 7,
            device_id: DEVICE
        })
    );
    assert!(
        bus.sent[1].0 - bus.sent[0].0 >= Duration::from_micros(round().response_window_end_us())
    );
}

#[tokio::test(start_paused = true)]
async fn responder_obeys_slot_and_ack_and_skips_late_rounds() {
    let mut bus = Bus::default();
    let mut participant = Participant::new(DEVICE);
    let observed = Instant::now();
    assert!(
        respond(
            &mut bus,
            &CODEC,
            &mut participant,
            &Message::Rs485DiscoveryRound(round()),
            observed,
            timing()
        )
        .await
        .unwrap()
    );
    let opens = u64::from(round().turnaround_us) + u64::from(round().slot_for(DEVICE)) * 10_000;
    let sent = bus.sent[0].0 - observed;
    assert!(sent >= Duration::from_micros(opens + u64::from(timing().receive_margin_us)));
    assert!(sent + Duration::from_micros(500) <= Duration::from_micros(opens + 10_000 - 100));
    let ack = Message::Rs485DiscoveryAck(DiscoveryAck {
        discovery_id: 7,
        device_id: DEVICE,
    });
    assert!(
        !respond(&mut bus, &CODEC, &mut participant, &ack, observed, timing())
            .await
            .unwrap()
    );
    let mut retry = round();
    retry.seed += 1;
    assert!(
        !respond(
            &mut bus,
            &CODEC,
            &mut participant,
            &Message::Rs485DiscoveryRound(retry),
            observed,
            timing()
        )
        .await
        .unwrap()
    );
    retry.discovery_id += 1;
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(
        !respond(
            &mut bus,
            &CODEC,
            &mut participant,
            &Message::Rs485DiscoveryRound(retry),
            observed,
            timing()
        )
        .await
        .unwrap()
    );
    assert_eq!(bus.sent.len(), 1);
}

#[tokio::test(start_paused = true)]
async fn stalled_sends_are_bounded_and_not_replayed_in_the_same_round() {
    let mut bus = Bus {
        stall: true,
        ..Default::default()
    };
    let mut participant = Participant::new(DEVICE);
    let observed = Instant::now();
    let message = Message::Rs485DiscoveryRound(round());
    assert!(matches!(
        respond(
            &mut bus,
            &CODEC,
            &mut participant,
            &message,
            observed,
            timing()
        )
        .await,
        Err(Error::TimedOut)
    ));
    bus.stall = false;
    assert!(
        !respond(
            &mut bus,
            &CODEC,
            &mut participant,
            &message,
            Instant::now(),
            timing()
        )
        .await
        .unwrap()
    );
    assert!(bus.sent.is_empty());
    bus.stall = true;
    assert!(matches!(
        discover::<1, _>(&mut bus, &CODEC, round(), Duration::from_millis(5), |_| {}).await,
        Err(Error::TimedOut)
    ));
}

#[tokio::test(start_paused = true)]
async fn participant_loop_dispatches_rounds_and_acks_without_replaying() {
    let mut retry = round();
    retry.seed += 1;
    let messages = [
        Message::Rs485DiscoveryRound(round()),
        Message::Rs485DiscoveryAck(DiscoveryAck {
            discovery_id: 7,
            device_id: DEVICE,
        }),
        Message::Rs485DiscoveryRound(retry),
    ];
    let mut bus = Bus {
        incoming: messages.into_iter().flat_map(wire).collect(),
        ..Default::default()
    };
    let mut participant = Participant::new(DEVICE);
    let mut storage = [0; WIRE_CAPACITY];
    let mut observed = Vec::new();
    assert!(
        tokio::time::timeout(
            Duration::from_secs(1),
            serve(
                &mut bus,
                &CODEC,
                &mut participant,
                timing(),
                &mut storage,
                |event| if let discovery_tokio::StreamEvent::Discovery(message) = event.unwrap() {
                    observed.push(message);
                }
            )
        )
        .await
        .is_err()
    );
    assert_eq!(observed, messages);
    assert_eq!(bus.sent.len(), 1);
    assert!(matches!(bus.sent[0].1, Message::Announcement(_)));
}
