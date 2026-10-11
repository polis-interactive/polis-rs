use core::num::{NonZeroU16, NonZeroU32};

use discovery::{
    Announcement, ApplicationId, DeviceId, Discover, Discovery, Error, MAX_PAYLOAD_LEN, Message,
    PROTOCOL_ID, SCHEMA_VERSION,
    rs485::{DiscoveryAck, DiscoveryRound},
};

const APPLICATION: ApplicationId = ApplicationId(*b"example-app-0001");
const CODEC: Discovery = Discovery::new(APPLICATION);

const DEVICE: DeviceId = DeviceId([1, 2, 3, 4, 5, 6, 7, 8]);

fn round() -> DiscoveryRound {
    DiscoveryRound {
        discovery_id: 300,
        seed: 128,
        slot_count: NonZeroU16::new(32).unwrap(),
        slot_duration_us: NonZeroU32::new(1000).unwrap(),
        turnaround_us: 200,
    }
}

// Written from the schema: u64/u32/u16 LEB128, followed by fixed ID bytes.
// Neither production encoding nor Serde enum positions generate these fixtures.
fn fixtures() -> [(u16, Message, &'static [u8]); 4] {
    [
        (
            0x0001,
            Message::Discover(Discover { discovery_id: 300 }),
            &[0xac, 2],
        ),
        (
            0x0002,
            Message::Announcement(Announcement {
                discovery_id: 300,
                device_id: DEVICE,
            }),
            &[0xac, 2, 1, 2, 3, 4, 5, 6, 7, 8],
        ),
        (
            0x0101,
            Message::Rs485DiscoveryRound(round()),
            &[0xac, 2, 0x80, 1, 32, 0xe8, 7, 0xc8, 1],
        ),
        (
            0x0102,
            Message::Rs485DiscoveryAck(DiscoveryAck {
                discovery_id: 300,
                device_id: DEVICE,
            }),
            &[0xac, 2, 1, 2, 3, 4, 5, 6, 7, 8],
        ),
    ]
}

fn decode(message_type: u16, bytes: &[u8]) -> Result<Message, Error> {
    let payload: Vec<u8> = APPLICATION
        .0
        .into_iter()
        .chain(bytes.iter().copied())
        .collect();
    CODEC.decode_payload(PROTOCOL_ID, SCHEMA_VERSION, message_type, &payload)
}

#[test]
fn golden_payloads_lock_ids_field_order_and_varints() {
    assert_eq!(PROTOCOL_ID, 0x5044);
    assert_eq!(SCHEMA_VERSION, 1);
    for (id, message, bytes) in fixtures() {
        assert_eq!(message.message_type(), id);
        let mut output = [0; MAX_PAYLOAD_LEN as usize];
        let expected: Vec<u8> = APPLICATION
            .0
            .into_iter()
            .chain(bytes.iter().copied())
            .collect();
        assert_eq!(
            CODEC.encode_payload(&message, &mut output).unwrap(),
            expected
        );
        assert_eq!(decode(id, bytes), Ok(message));
    }
}

#[test]
fn rejects_truncated_trailing_and_unknown_payloads() {
    for (id, _, bytes) in fixtures() {
        for end in 0..bytes.len() {
            assert!(decode(id, &bytes[..end]).is_err(), "id={id}, end={end}");
        }
        let mut extra = bytes.to_vec();
        extra.push(0);
        assert_eq!(decode(id, &extra), Err(Error::TrailingPayload));
    }
    assert_eq!(
        CODEC.decode_payload(0xfffe, 1, 1, &[]),
        Err(Error::UnknownProtocol(0xfffe))
    );
    assert_eq!(
        CODEC.decode_payload(PROTOCOL_ID, 2, 1, &[]),
        Err(Error::UnsupportedSchema(2))
    );
    assert_eq!(decode(0x9999, &[]), Err(Error::UnknownMessageType(0x9999)));
    assert!(matches!(decode(1, &[0xff; 11]), Err(Error::Payload(_))));
}

#[test]
fn zero_ids_only_allow_unsolicited_announcements_and_zero_slots_are_invalid() {
    for (id, message, bytes) in fixtures() {
        let mut zero_id = vec![0];
        zero_id.extend_from_slice(&bytes[2..]);
        if matches!(message, Message::Announcement(_)) {
            assert_eq!(
                decode(id, &zero_id),
                Ok(Message::Announcement(Announcement {
                    discovery_id: 0,
                    device_id: DEVICE,
                }))
            );
        } else {
            assert_eq!(decode(id, &zero_id), Err(Error::ZeroDiscoveryId));
        }
    }
    for bytes in [&[1, 0, 0, 1, 0][..], &[1, 0, 1, 0, 0][..]] {
        assert!(matches!(decode(0x0101, bytes), Err(Error::Payload(_))));
    }
    let mut output = [0; MAX_PAYLOAD_LEN as usize];
    for message in [
        Message::Discover(Discover { discovery_id: 0 }),
        Message::Rs485DiscoveryRound(DiscoveryRound {
            discovery_id: 0,
            ..round()
        }),
        Message::Rs485DiscoveryAck(DiscoveryAck {
            discovery_id: 0,
            device_id: DEVICE,
        }),
    ] {
        assert_eq!(
            CODEC.encode_payload(&message, &mut output),
            Err(Error::ZeroDiscoveryId)
        );
    }
}

#[test]
fn maximum_values_fit_the_documented_payload_budget() {
    let messages = [
        Message::Discover(Discover {
            discovery_id: u64::MAX,
        }),
        Message::Announcement(Announcement {
            discovery_id: u64::MAX,
            device_id: DeviceId([255; 8]),
        }),
        Message::Rs485DiscoveryRound(DiscoveryRound {
            discovery_id: u64::MAX,
            seed: u32::MAX,
            slot_count: NonZeroU16::MAX,
            slot_duration_us: NonZeroU32::MAX,
            turnaround_us: u32::MAX,
        }),
        Message::Rs485DiscoveryAck(DiscoveryAck {
            discovery_id: u64::MAX,
            device_id: DeviceId([255; 8]),
        }),
    ];
    for (message, expected_len) in messages.into_iter().zip([26, 34, 44, 34]) {
        let mut output = [0; MAX_PAYLOAD_LEN as usize];
        let bytes = CODEC.encode_payload(&message, &mut output).unwrap();
        assert_eq!(bytes.len(), expected_len);
        assert_eq!(
            CODEC.decode_payload(PROTOCOL_ID, SCHEMA_VERSION, message.message_type(), bytes),
            Ok(message)
        );
        assert!(matches!(
            CODEC.encode_payload(&message, &mut output[..expected_len - 1]),
            Err(Error::Payload(_))
        ));
    }
}

#[test]
fn fixed_slot_vectors_mix_seed_bits_for_power_of_two_windows() {
    // Independent Python u32 calculation; the second ID aliases the first in
    // the low five bits of unfinalized FNV for every seed.
    let second = DeviceId([1, 2, 3, 4, 5, 6, 7, 40]);
    for (seed, expected) in [
        (0, [12, 22]),
        (1, [16, 27]),
        (2, [18, 26]),
        (0x12345678, [17, 2]),
    ] {
        let round = DiscoveryRound { seed, ..round() };
        assert_eq!([round.slot_for(DEVICE), round.slot_for(second)], expected);
    }
    for count in [1, 3, 32, u16::MAX] {
        let round = DiscoveryRound {
            slot_count: NonZeroU16::new(count).unwrap(),
            ..round()
        };
        assert!(round.slot_for(DEVICE) < count);
    }
}

#[test]
fn response_window_uses_wide_arithmetic() {
    assert_eq!(round().response_window_end_us(), 32_200);
    let largest = DiscoveryRound {
        slot_count: NonZeroU16::MAX,
        slot_duration_us: NonZeroU32::MAX,
        turnaround_us: u32::MAX,
        ..round()
    };
    assert_eq!(largest.response_window_end_us(), 281_474_976_645_120);
}

#[test]
fn same_announcement_composes_with_stream_and_datagram_framing() {
    use framing::{Decoder, Header, decode_datagram, encode, encode_datagram, wire_capacity};

    let message = Message::Announcement(Announcement {
        discovery_id: 300,
        device_id: DEVICE,
    });
    let header = Header {
        protocol_id: PROTOCOL_ID,
        schema_version: SCHEMA_VERSION,
        message_type: message.message_type(),
    };
    let mut payload = [0; MAX_PAYLOAD_LEN as usize];
    let bytes = CODEC.encode_payload(&message, &mut payload).unwrap();
    let mut wire = [0; wire_capacity(MAX_PAYLOAD_LEN)];
    let stream = encode(header, bytes, &mut wire).unwrap();
    let mut rx = [0; wire_capacity(MAX_PAYLOAD_LEN)];
    let mut decoder = Decoder::new(&mut rx);
    let mut received = Vec::new();
    for part in stream.chunks(3) {
        decoder.feed(part, |event| {
            let frame = event.unwrap();
            received.push(
                CODEC
                    .decode_payload(
                        frame.header.protocol_id,
                        frame.header.schema_version,
                        frame.header.message_type,
                        frame.payload,
                    )
                    .unwrap(),
            );
        });
    }
    assert_eq!(received, [message]);

    let mut datagram = [0; wire_capacity(MAX_PAYLOAD_LEN)];
    let budget = datagram.len();
    let udp = encode_datagram(header, bytes, budget, &mut datagram).unwrap();
    assert_eq!(udp, stream);
    let len = udp.len();
    let frame = decode_datagram(&mut datagram[..len], budget).unwrap();
    assert_eq!(
        CODEC.decode_payload(
            frame.header.protocol_id,
            frame.header.schema_version,
            frame.header.message_type,
            frame.payload
        ),
        Ok(message)
    );
}
