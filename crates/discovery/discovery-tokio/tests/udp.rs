#![cfg(feature = "udp")]
use polis_discovery_protocol::{
    Announcement, ApplicationId, DeviceId, Discover, Discovery, Message,
};
use polis_discovery_tokio::{Error, WIRE_CAPACITY, udp};
use std::{num::NonZeroU64, time::Duration};
use tokio::net::UdpSocket;

const CODEC: Discovery = Discovery::new(ApplicationId(*b"example-app-0001"));
const REPLY: Message = Message::Announcement(Announcement {
    discovery_id: 17,
    device_id: DeviceId(*b"device01"),
});

#[tokio::test]
async fn probe_reply_preserves_identity_and_actual_source_port() {
    let host = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let device = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let query = Message::Discover(Discover { discovery_id: 17 });
    udp::send(&host, &CODEC, &query, device.local_addr().unwrap())
        .await
        .unwrap();
    let (message, peer) = udp::receive(&device, &CODEC).await.unwrap();
    assert_eq!(message, query);
    assert_eq!(peer, host.local_addr().unwrap());
    udp::send(&device, &CODEC, &REPLY, peer).await.unwrap();
    let (message, peer) = udp::receive(&host, &CODEC).await.unwrap();
    assert_eq!(message, REPLY);
    assert_eq!(peer, device.local_addr().unwrap());
}

#[tokio::test]
async fn bad_datagrams_do_not_poison_the_next_receive() {
    let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = receiver.local_addr().unwrap();
    let mut scratch = [0; polis_discovery_protocol::MAX_PAYLOAD_LEN as usize];
    let mut wire = [0; WIRE_CAPACITY];
    let valid = polis_framing::encode(
        polis_framing::Header {
            protocol_id: polis_discovery_protocol::PROTOCOL_ID,
            schema_version: polis_discovery_protocol::SCHEMA_VERSION,
            message_type: REPLY.message_type(),
        },
        CODEC.encode_payload(&REPLY, &mut scratch).unwrap(),
        &mut wire,
    )
    .unwrap()
    .to_vec();
    let mut padded = valid.clone();
    padded.resize(WIRE_CAPACITY + 200, 0); // valid prefix must not pass after socket truncation
    let doubled = [valid.as_slice(), valid.as_slice()].concat();
    for invalid in [
        &valid[..valid.len() - 1],
        padded.as_slice(),
        doubled.as_slice(),
        &[],
    ] {
        sender.send_to(invalid, address).await.unwrap();
        assert!(matches!(
            udp::receive(&receiver, &CODEC).await,
            Err(Error::Framing(_))
        ));
    }
    udp::send(
        &sender,
        &Discovery::new(ApplicationId([0; 16])),
        &REPLY,
        address,
    )
    .await
    .unwrap();
    assert!(matches!(
        udp::receive(&receiver, &CODEC).await,
        Err(Error::Protocol(
            polis_discovery_protocol::Error::WrongApplication(_)
        ))
    ));
    udp::send(&sender, &CODEC, &REPLY, address).await.unwrap();
    assert_eq!(udp::receive(&receiver, &CODEC).await.unwrap().0, REPLY);
}

#[tokio::test]
async fn scan_filters_unrelated_responses_and_stops_at_its_deadline() {
    let host = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let device = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let destination = device.local_addr().unwrap();
    let responder = tokio::spawn(async move {
        let (message, peer) = udp::receive(&device, &CODEC).await.unwrap();
        assert_eq!(message, Message::Discover(Discover { discovery_id: 17 }));
        for id in [0, 16, 17, 18] {
            let message = Message::Announcement(Announcement {
                discovery_id: id,
                device_id: DeviceId(*b"device01"),
            });
            udp::send(&device, &CODEC, &message, peer).await.unwrap();
        }
        udp::send(
            &device,
            &Discovery::new(ApplicationId([0; 16])),
            &REPLY,
            peer,
        )
        .await
        .unwrap();
        // Keep unrelated traffic arriving beyond the scan's response window.
        loop {
            device.send_to(b"noise", peer).await.unwrap();
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    });
    let mut found = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(2),
        udp::discover(
            &host,
            &CODEC,
            NonZeroU64::new(17).unwrap(),
            destination,
            Duration::from_millis(60),
            |value, peer| found.push((value, peer)),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    responder.abort();
    assert_eq!(
        found,
        [(
            Announcement {
                discovery_id: 17,
                device_id: DeviceId(*b"device01")
            },
            destination
        )]
    );
}

#[tokio::test]
async fn continuous_roles_support_unsolicited_announcements_and_probe_replies() {
    tokio::time::timeout(Duration::from_secs(2), async {
        let host = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let device = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let destination = device.local_addr().unwrap();
        let host_address = host.local_addr().unwrap();
        let mut observed = Vec::new();
        let client = async {
            host.send_to(b"noise", destination).await.unwrap();
            udp::announce(&host, &Discovery::new(ApplicationId([0; 16])), DeviceId([0; 8]), destination).await.unwrap();
            udp::announce(&host, &CODEC, DeviceId(*b"host0001"), destination).await.unwrap();
            let mut found = Vec::new();
            udp::discover(&host, &CODEC, NonZeroU64::new(99).unwrap(), destination, Duration::from_millis(40), |value, peer| found.push((value, peer))).await.unwrap();
            assert_eq!(found, [(Announcement { discovery_id: 99, device_id: DeviceId(*b"device01") }, destination)]);
        };
        tokio::select! {
            result = udp::serve(&device, &CODEC, Some(DeviceId(*b"device01")), |value, peer| observed.push((value, peer))) => panic!("server stopped: {result:?}"),
            () = client => {},
        }
        assert_eq!(observed, [(Announcement::unsolicited(DeviceId(*b"host0001")), host_address)]);
        let mut passive = Vec::new();
        tokio::select! {
            result = udp::listen(&host, &CODEC, |value, peer| passive.push((value, peer))) => panic!("listener stopped: {result:?}"),
            () = async {
                udp::announce(&device, &CODEC, DeviceId(*b"device01"), host_address).await.unwrap();
                tokio::time::sleep(Duration::from_millis(20)).await;
            } => {},
        }
        assert_eq!(passive, [(Announcement::unsolicited(DeviceId(*b"device01")), destination)]);
    }).await.unwrap();
}
