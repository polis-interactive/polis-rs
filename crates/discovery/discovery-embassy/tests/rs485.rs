#![cfg(feature = "rs485")]
use discovery_embassy::{WIRE_CAPACITY, rs485::*};
use discovery_protocol::{Announcement, ApplicationId, DeviceId, Discovery, Message};
use embassy_time::{Duration, Instant};
use embedded_io_async::{ErrorType, Read, Write};
use std::{
    convert::Infallible,
    num::{NonZeroU16, NonZeroU32},
};
const CODEC: Discovery = Discovery::new(ApplicationId(*b"example-app-0001"));
const DEVICE: DeviceId = DeviceId(*b"device01");
#[derive(Default)]
struct Bus {
    incoming: Vec<u8>,
    cursor: usize,
    sent: Vec<(Instant, Message)>,
}
impl ErrorType for Bus {
    type Error = Infallible;
}
impl Read for Bus {
    async fn read(&mut self, out: &mut [u8]) -> Result<usize, Infallible> {
        if self.cursor == self.incoming.len() {
            return std::future::pending().await;
        }
        let n = 3.min(out.len()).min(self.incoming.len() - self.cursor);
        out[..n].copy_from_slice(&self.incoming[self.cursor..self.cursor + n]);
        self.cursor += n;
        Ok(n)
    }
}
impl Write for Bus {
    async fn write(&mut self, bytes: &[u8]) -> Result<usize, Infallible> {
        let mut copy = bytes.to_vec();
        let frame = framing::decode_datagram(&mut copy, WIRE_CAPACITY).unwrap();
        let message = CODEC
            .decode_payload(
                frame.header.protocol_id,
                frame.header.schema_version,
                frame.header.message_type,
                frame.payload,
            )
            .unwrap();
        self.sent.push((Instant::now(), message));
        Ok(bytes.len())
    }
    async fn flush(&mut self) -> Result<(), Infallible> {
        Ok(())
    }
}
fn wire(message: Message) -> Vec<u8> {
    let mut payload = [0; discovery_protocol::MAX_PAYLOAD_LEN as usize];
    let mut out = [0; WIRE_CAPACITY];
    framing::encode(
        framing::Header {
            protocol_id: discovery_protocol::PROTOCOL_ID,
            schema_version: 1,
            message_type: message.message_type(),
        },
        CODEC.encode_payload(&message, &mut payload).unwrap(),
        &mut out,
    )
    .unwrap()
    .to_vec()
}
#[tokio::test]
async fn embassy_clock_schedules_response_and_delays_controller_ack() {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let round = DiscoveryRound {
            discovery_id: 7,
            seed: 1,
            slot_count: NonZeroU16::new(2).unwrap(),
            slot_duration_us: NonZeroU32::new(100_000).unwrap(),
            turnaround_us: 1000,
        };
        let reply = Announcement {
            discovery_id: 7,
            device_id: DEVICE,
        };
        let mut bus = Bus {
            incoming: wire(Message::Announcement(reply)),
            ..Default::default()
        };
        let mut found = Vec::new();
        assert_eq!(
            discover::<2, _>(&mut bus, &CODEC, round, Duration::from_secs(1), |a| found
                .push(a))
            .await
            .unwrap(),
            ScanSummary {
                found: 1,
                overflow: false
            }
        );
        assert_eq!(found, [reply]);
        assert!(
            bus.sent[1].0 - bus.sent[0].0 >= Duration::from_micros(round.response_window_end_us())
        );
        let ack = bus.sent[1].1;
        let mut participant = Participant::new(DEVICE);
        let timing = Timing {
            receive_margin_us: 100,
            transmit_budget_us: NonZeroU32::new(500).unwrap(),
        };
        let observed = Instant::now();
        assert!(
            respond(
                &mut bus,
                &CODEC,
                &mut participant,
                &Message::Rs485DiscoveryRound(round),
                observed,
                timing
            )
            .await
            .unwrap()
        );
        let sent = bus.sent[2].0;
        let opens = u64::from(round.turnaround_us) + u64::from(round.slot_for(DEVICE)) * 100_000;
        assert!(sent >= observed + Duration::from_micros(opens));
        assert!(
            sent + Duration::from_micros(500)
                <= observed + Duration::from_micros(opens + 100_000 - 100)
        );
        assert!(
            !respond(&mut bus, &CODEC, &mut participant, &ack, observed, timing)
                .await
                .unwrap()
        );
        let retry = DiscoveryRound { seed: 2, ..round };
        assert!(
            !respond(
                &mut bus,
                &CODEC,
                &mut participant,
                &Message::Rs485DiscoveryRound(retry),
                Instant::now(),
                timing
            )
            .await
            .unwrap()
        );
        assert_eq!(bus.sent.len(), 3);
        let messages = [
            Message::Rs485DiscoveryRound(round),
            ack,
            Message::Rs485DiscoveryRound(retry),
        ];
        let mut bus = Bus {
            incoming: messages.into_iter().flat_map(wire).collect(),
            ..Default::default()
        };
        let mut participant = Participant::new(DEVICE);
        let mut storage = [0; WIRE_CAPACITY];
        let mut seen = Vec::new();
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(250),
                serve(
                    &mut bus,
                    &CODEC,
                    &mut participant,
                    timing,
                    &mut storage,
                    |event| {
                        if let discovery_embassy::StreamEvent::Discovery(message) = event.unwrap() {
                            seen.push(message);
                        }
                    }
                )
            )
            .await
            .is_err()
        );
        assert_eq!(seen, messages);
        assert_eq!(bus.sent.len(), 1);
    })
    .await
    .unwrap();
}
