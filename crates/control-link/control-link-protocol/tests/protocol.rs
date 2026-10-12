use core::num::{NonZeroU32, NonZeroU64};

use polis_control_link_protocol::{
    ControlLink, Error, Incoming, Key, MAX_BODY_LEN, Message, PREFIX_LEN, PROTOCOL_ID, RequestGate,
    Requests, SCHEMA_VERSION, StateError, message_type,
};
use polis_framing::{Decoder, Frame, Header, wire_capacity};

const APP: [u8; 16] = *b"example-app-0001";
const CODEC: ControlLink = ControlLink::new(APP);
const KEY: Key = Key {
    session: NonZeroU64::new(0x0807060504030201).unwrap(),
    id: NonZeroU32::new(0x0c0b0a09).unwrap(),
};

fn frame(kind: u16, payload: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            protocol_id: PROTOCOL_ID,
            schema_version: SCHEMA_VERSION,
            message_type: kind,
        },
        payload,
    }
}

#[test]
fn fixed_payload_and_header_assignments_define_the_wire_schema() {
    let expected = b"example-app-0001\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x00\xff";
    let message = Message::Request {
        key: KEY,
        body: &[0, 255],
    };
    let mut scratch = [0; PREFIX_LEN + 2];
    let mut wire = [0; wire_capacity((PREFIX_LEN + 2) as u16)];
    let bytes = CODEC.encode(message, &mut scratch, &mut wire).unwrap();
    let decoded = polis_framing::decode_datagram(&mut bytes.to_vec(), wire.len())
        .unwrap()
        .payload
        .to_vec();
    assert_eq!(decoded, expected);
    assert_eq!(CODEC.decode(frame(1, expected)), Ok(message));
    assert_eq!(PROTOCOL_ID, 0x5043);
    assert_eq!(SCHEMA_VERSION, 1);
    assert_eq!(
        (
            message_type::REQUEST,
            message_type::ACCEPTED,
            message_type::RESPONSE,
            message_type::EVENT
        ),
        (1, 2, 3, 4)
    );
}

#[test]
fn all_message_kinds_survive_every_stream_split() {
    let messages = [
        Message::Request {
            key: KEY,
            body: b"command",
        },
        Message::Accepted { key: KEY },
        Message::Event {
            session: KEY.session,
            body: b"telemetry",
        },
        Message::Response {
            key: KEY,
            body: b"result",
        },
    ];
    let mut bytes = Vec::new();
    for message in messages {
        let mut scratch = [0; 64];
        let mut wire = [0; wire_capacity(64)];
        bytes.extend_from_slice(CODEC.encode(message, &mut scratch, &mut wire).unwrap());
    }
    for split in 0..=bytes.len() {
        let mut storage = [0; wire_capacity(64)];
        let mut decoder = Decoder::new(&mut storage);
        let mut index = 0;
        for chunk in [&bytes[..split], &bytes[split..]] {
            decoder.feed(chunk, |result| {
                assert_eq!(CODEC.decode(result.unwrap()).unwrap(), messages[index]);
                index += 1;
            });
        }
        decoder.finish().unwrap();
        assert_eq!(index, messages.len());
    }
}

#[test]
fn bad_envelopes_are_rejected_before_application_dispatch() {
    let mut payload = *b"example-app-0001\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c";
    for len in 0..PREFIX_LEN {
        assert_eq!(
            CODEC.decode(frame(1, &payload[..len])),
            Err(Error::Truncated)
        );
    }
    let mut other = frame(1, &payload);
    other.header.protocol_id = 123;
    assert_eq!(CODEC.decode(other), Err(Error::UnknownProtocol(123)));
    other.header.protocol_id = PROTOCOL_ID;
    other.header.schema_version = 2;
    assert_eq!(CODEC.decode(other), Err(Error::UnsupportedSchema(2)));
    assert_eq!(
        CODEC.decode(frame(999, &payload)),
        Err(Error::UnknownMessageType(999))
    );
    assert_eq!(
        ControlLink::new([0; 16]).decode(frame(1, &payload[..16])),
        Err(Error::WrongApplication)
    );
    assert_eq!(
        CODEC.decode(frame(4, &payload)),
        Err(Error::EventHasRequestId)
    );
    payload[24..28].fill(0);
    assert_eq!(CODEC.decode(frame(1, &payload)), Err(Error::ZeroRequestId));
    assert_eq!(
        CODEC.decode(frame(4, &payload)),
        Ok(Message::Event {
            session: KEY.session,
            body: &[]
        })
    );
    payload[16..24].fill(0);
    assert_eq!(CODEC.decode(frame(4, &payload)), Err(Error::ZeroSession));

    let accepted = b"example-app-0001\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0cX";
    assert_eq!(
        CODEC.decode(frame(2, accepted)),
        Err(Error::AcceptedHasBody)
    );
}

#[test]
fn body_and_buffer_limits_are_enforced() {
    let body = vec![0xab; MAX_BODY_LEN];
    let mut scratch = vec![0; u16::MAX as usize];
    let mut wire = vec![0; wire_capacity(u16::MAX)];
    let message = Message::Request {
        key: KEY,
        body: &body,
    };
    let bytes = CODEC.encode(message, &mut scratch, &mut wire).unwrap();
    let mut storage = bytes.to_vec();
    let decoded = polis_framing::decode_datagram(&mut storage, wire_capacity(u16::MAX)).unwrap();
    assert_eq!(CODEC.decode(decoded).unwrap(), message);
    let oversized = vec![0; MAX_BODY_LEN + 1];
    assert_eq!(
        CODEC.encode(
            Message::Request {
                key: KEY,
                body: &oversized
            },
            &mut scratch,
            &mut wire
        ),
        Err(Error::BodyTooLarge)
    );

    let message = Message::Request {
        key: KEY,
        body: b"x",
    };
    for len in 0..PREFIX_LEN + 1 {
        assert_eq!(
            CODEC.encode(message, &mut scratch[..len], &mut wire),
            Err(Error::OutputTooSmall)
        );
    }
    for len in 0..wire_capacity((PREFIX_LEN + 1) as u16) {
        assert_eq!(
            CODEC.encode(message, &mut scratch, &mut wire[..len]),
            Err(Error::Framing(polis_framing::Error::OutputTooSmall))
        );
    }
}

#[test]
fn long_operation_events_and_keepalive_share_a_session() {
    let mut requests = Requests::<2>::new(KEY.session);
    let mut gate = RequestGate::new(KEY.session);
    let operation = requests.reserve().unwrap();
    gate.accept(operation).unwrap();
    assert_eq!(
        requests.receive(Message::Accepted { key: operation }),
        Ok(Incoming::Accepted { key: operation })
    );
    let ping = requests.reserve().unwrap();
    gate.accept(ping).unwrap();
    assert_eq!(requests.reserve(), Err(StateError::Full));
    assert_eq!(
        requests.receive(Message::Event {
            session: KEY.session,
            body: b"data"
        }),
        Ok(Incoming::Event { body: b"data" })
    );
    assert_eq!(requests.pending_count(), 2);
    assert_eq!(
        requests.receive(Message::Response {
            key: ping,
            body: b"alive"
        }),
        Ok(Incoming::Response {
            key: ping,
            body: b"alive"
        })
    );
    assert_eq!(requests.pending_count(), 1);
    assert_eq!(
        requests.receive(Message::Response {
            key: operation,
            body: b"done"
        }),
        Ok(Incoming::Response {
            key: operation,
            body: b"done"
        })
    );
    assert_eq!(requests.pending_count(), 0);
    assert_eq!(gate.accept(operation), Err(StateError::DuplicateRequest));
}

#[test]
fn cancellations_and_completed_requests_cannot_capture_later_replies() {
    let mut requests = Requests::<1>::new(KEY.session);
    let first = requests.reserve().unwrap();
    requests.cancel(first).unwrap();
    let second = requests.reserve().unwrap();
    assert_eq!(second.id.get(), first.id.get() + 1);
    assert_eq!(
        requests.receive(Message::Response {
            key: first,
            body: b"late"
        }),
        Err(StateError::UnknownRequest)
    );
    assert_eq!(
        requests.receive(Message::Accepted { key: first }),
        Err(StateError::UnknownRequest)
    );
    assert_eq!(requests.cancel(first), Err(StateError::UnknownRequest));
    assert_eq!(requests.pending_count(), 1);
    requests
        .receive(Message::Response {
            key: second,
            body: b"done",
        })
        .unwrap();
    assert_eq!(
        requests.receive(Message::Response {
            key: second,
            body: b"duplicate"
        }),
        Err(StateError::UnknownRequest)
    );
    assert_eq!(
        Requests::<0>::new(KEY.session).reserve(),
        Err(StateError::Full)
    );
}

#[test]
fn old_sessions_and_incoming_requests_do_not_change_pending_calls() {
    let mut requests = Requests::<1>::new(NonZeroU64::new(42).unwrap());
    let current = requests.reserve().unwrap();
    for message in [
        Message::Response {
            key: KEY,
            body: b"old",
        },
        Message::Accepted { key: KEY },
        Message::Event {
            session: KEY.session,
            body: b"old",
        },
    ] {
        assert_eq!(requests.receive(message), Err(StateError::WrongSession));
    }
    assert_eq!(requests.cancel(KEY), Err(StateError::WrongSession));
    assert_eq!(
        requests.receive(Message::Request {
            key: current,
            body: b"peer request"
        }),
        Err(StateError::UnexpectedRequest)
    );
    assert_eq!(requests.pending_count(), 1);
    let mut gate = RequestGate::new(current.session);
    assert_eq!(gate.accept(KEY), Err(StateError::WrongSession));
    gate.accept(current).unwrap();
    assert_eq!(gate.accept(current), Err(StateError::DuplicateRequest));
}

#[test]
fn one_decoder_routes_discovery_and_control_without_a_dependency_between_protocols() {
    use polis_discovery_protocol::{
        ApplicationId, Discover, Discovery, Message as DiscoveryMessage,
    };

    let discovery = Discovery::new(ApplicationId(APP));
    let query = DiscoveryMessage::Discover(Discover { discovery_id: 1 });
    let mut payload = [0; 64];
    let mut wire = [0; wire_capacity(64)];
    let mut bytes = polis_framing::encode(
        Header {
            protocol_id: polis_discovery_protocol::PROTOCOL_ID,
            schema_version: polis_discovery_protocol::SCHEMA_VERSION,
            message_type: query.message_type(),
        },
        discovery.encode_payload(&query, &mut payload).unwrap(),
        &mut wire,
    )
    .unwrap()
    .to_vec();
    bytes.extend_from_slice(
        CODEC
            .encode(
                Message::Request {
                    key: KEY,
                    body: b"business",
                },
                &mut payload,
                &mut wire,
            )
            .unwrap(),
    );
    let mut decoder_storage = [0; wire_capacity(64)];
    let mut decoder = Decoder::new(&mut decoder_storage);
    let mut discoveries = 0;
    let mut requests = 0;
    for chunk in bytes.chunks(3) {
        decoder.feed(chunk, |result| {
            let frame = result.unwrap();
            match frame.header.protocol_id {
                polis_discovery_protocol::PROTOCOL_ID => {
                    assert_eq!(
                        discovery.decode_payload(
                            frame.header.protocol_id,
                            frame.header.schema_version,
                            frame.header.message_type,
                            frame.payload
                        ),
                        Ok(query)
                    );
                    discoveries += 1;
                }
                PROTOCOL_ID => {
                    assert_eq!(
                        CODEC.decode(frame),
                        Ok(Message::Request {
                            key: KEY,
                            body: b"business"
                        })
                    );
                    requests += 1;
                }
                other => panic!("unexpected protocol {other}"),
            }
        });
    }
    assert_eq!((discoveries, requests), (1, 1));
}
