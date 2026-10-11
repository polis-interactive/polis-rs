use discovery_protocol::{
    APPLICATION_ID_LEN, ApplicationId, DeviceId, Discover, Discovery, Error, MAX_PAYLOAD_LEN,
    Message, PROTOCOL_ID, SCHEMA_VERSION,
};

const APP: ApplicationId = ApplicationId(*b"example-app-0001");
const OTHER: ApplicationId = ApplicationId(*b"example-app-0002");

#[test]
fn independently_configured_peers_interoperate_and_other_apps_are_filtered() {
    let sender = Discovery::new(APP);
    let receiver = Discovery::new(ApplicationId(*b"example-app-0001"));
    let unrelated = Discovery::new(OTHER);
    let message = Message::Discover(Discover { discovery_id: 300 });
    let mut buffer = [0; MAX_PAYLOAD_LEN as usize];
    let bytes = sender.encode_payload(&message, &mut buffer).unwrap();
    // Fixed prefix plus independently written unsigned LEB128 for 300.
    assert_eq!(bytes, b"example-app-0001\xac\x02");
    assert_eq!(
        receiver.decode_payload(PROTOCOL_ID, SCHEMA_VERSION, 1, bytes),
        Ok(message)
    );
    assert_eq!(
        unrelated.decode_payload(PROTOCOL_ID, SCHEMA_VERSION, 1, bytes),
        Err(Error::WrongApplication(APP))
    );
}

#[test]
fn application_is_checked_before_body_and_missing_prefix_is_rejected() {
    let receiver = Discovery::new(APP);
    for length in 0..APPLICATION_ID_LEN {
        assert_eq!(
            receiver.decode_payload(PROTOCOL_ID, SCHEMA_VERSION, 1, &APP.0[..length]),
            Err(Error::TruncatedApplicationId)
        );
    }
    assert_eq!(
        receiver.decode_payload(PROTOCOL_ID, SCHEMA_VERSION, 1, &OTHER.0),
        Err(Error::WrongApplication(OTHER))
    );
    assert!(matches!(
        receiver.decode_payload(PROTOCOL_ID, SCHEMA_VERSION, 1, &APP.0),
        Err(Error::Payload(_))
    ));
}

#[test]
fn zero_application_id_is_an_ordinary_namespace_not_a_wildcard() {
    let zero = Discovery::new(ApplicationId([0; 16]));
    let message = Message::Announcement(discovery_protocol::Announcement {
        discovery_id: 0,
        device_id: DeviceId([0; 8]),
    });
    let mut buffer = [0; MAX_PAYLOAD_LEN as usize];
    let payload = zero.encode_payload(&message, &mut buffer).unwrap();
    assert_eq!(
        zero.decode_payload(PROTOCOL_ID, SCHEMA_VERSION, 2, payload),
        Ok(message)
    );
    assert_eq!(
        Discovery::new(APP).decode_payload(PROTOCOL_ID, SCHEMA_VERSION, 2, payload),
        Err(Error::WrongApplication(ApplicationId([0; 16])))
    );
}

#[test]
fn every_small_output_buffer_returns_an_error() {
    let codec = Discovery::new(APP);
    let message = Message::Discover(Discover { discovery_id: 300 });
    for size in 0..18 {
        assert!(codec.encode_payload(&message, &mut vec![0; size]).is_err());
    }
}
