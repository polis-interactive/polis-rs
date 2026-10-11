use control_link_tokio::{
    Error,
    protocol::{ControlLink, Incoming, Message, RequestGate, Requests},
    stream::{Receiver, Sender},
};
use std::{num::NonZeroU64, time::Duration};
use tokio::io::{AsyncWriteExt, duplex};
const LINK: ControlLink = ControlLink::new(*b"example-app-0001");
fn session() -> NonZeroU64 {
    NonZeroU64::new(23).unwrap()
}
fn wire(message: Message<'_>) -> Vec<u8> {
    LINK.encode(message, &mut [0; 128], &mut [0; 160])
        .unwrap()
        .to_vec()
}

#[tokio::test]
async fn handshake_and_coalesced_messages_share_decoder_and_pending_requests() {
    let (mut peer, stream) = duplex(1024);
    let mut storage = [0; 160];
    let mut rx = Receiver::new(LINK, stream, &mut storage);
    let mut requests = Requests::<2>::new(session());
    let hello = requests.reserve().unwrap();
    let command = requests.reserve().unwrap();
    let mut bytes = wire(Message::Response {
        key: hello,
        body: b"identity",
    });
    bytes.extend(wire(Message::Accepted { key: command }));
    bytes.extend(wire(Message::Event {
        session: session(),
        body: b"status",
    }));
    bytes.extend(wire(Message::Response {
        key: command,
        body: b"done",
    }));
    peer.write_all(&bytes).await.unwrap();
    let identity = rx
        .receive(|m| match requests.receive(m).unwrap() {
            Incoming::Response { key, body } => {
                assert_eq!(key, hello);
                body.to_vec()
            }
            _ => panic!(),
        })
        .await
        .unwrap();
    assert_eq!(identity, b"identity");
    assert_eq!(requests.pending_count(), 1);
    rx.receive(|m| {
        assert!(
            matches!(requests.receive(m).unwrap(), Incoming::Accepted { key } if key == command)
        )
    })
    .await
    .unwrap();
    rx.receive(|m| {
        assert_eq!(
            requests.receive(m).unwrap(),
            Incoming::Event { body: b"status" }
        )
    })
    .await
    .unwrap();
    rx.receive(|m| {
        assert_eq!(
            requests.receive(m).unwrap(),
            Incoming::Response {
                key: command,
                body: b"done"
            }
        )
    })
    .await
    .unwrap();
    assert_eq!(requests.pending_count(), 0);
}

#[tokio::test]
async fn receive_cancellation_retains_partial_frame_and_request_gate_rejects_duplicates() {
    let (mut peer, stream) = duplex(1024);
    let mut storage = [0; 160];
    let mut rx = Receiver::new(LINK, stream, &mut storage);
    let key = Requests::<1>::new(session()).reserve().unwrap();
    let frame = wire(Message::Request {
        key,
        body: b"hello",
    });
    peer.write_all(&frame[..7]).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(1), rx.receive(|_| ()))
            .await
            .is_err()
    );
    peer.write_all(&frame[7..]).await.unwrap();
    peer.write_all(&frame).await.unwrap();
    let mut gate = RequestGate::new(session());
    rx.receive(|m| {
        assert_eq!(
            m,
            Message::Request {
                key,
                body: b"hello"
            }
        );
        gate.accept(key).unwrap();
    })
    .await
    .unwrap();
    rx.receive(|_| {
        assert_eq!(
            gate.accept(key),
            Err(control_link_tokio::protocol::StateError::DuplicateRequest)
        )
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn cancelled_send_retires_writer_without_replaying() {
    let (_peer, stream) = duplex(1);
    let (mut payload, mut wire) = ([0; 128], [0; 160]);
    let mut tx = Sender::new(LINK, stream, &mut payload, &mut wire);
    let event = Message::Event {
        session: session(),
        body: b"event",
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(1), tx.send(event))
            .await
            .is_err()
    );
    assert!(matches!(tx.send(event).await, Err(Error::Interrupted)));
}

#[tokio::test]
async fn encoding_failure_is_retryable_and_small_writes_complete() {
    let (peer, stream) = duplex(3);
    let (mut payload, mut wire, mut storage) = ([0; 32], [0; 160], [0; 160]);
    let mut tx = Sender::new(LINK, stream, &mut payload, &mut wire);
    assert!(matches!(
        tx.send(Message::Event {
            session: session(),
            body: b"too large"
        })
        .await,
        Err(Error::Protocol(_))
    ));
    let mut rx = Receiver::new(LINK, peer, &mut storage);
    let event = Message::Event {
        session: session(),
        body: b"fits",
    };
    let (sent, received) = tokio::join!(tx.send(event), rx.receive(|m| assert_eq!(m, event)));
    sent.unwrap();
    received.unwrap();
}

#[tokio::test]
async fn eof_reports_truncation_once_and_foreign_frame_does_not_eat_next_message() {
    let (mut peer, stream) = duplex(1024);
    let mut storage = [0; 160];
    let mut rx = Receiver::new(LINK, stream, &mut storage);
    let event = Message::Event {
        session: session(),
        body: b"event",
    };
    let wrong = ControlLink::new(*b"other---app-0001")
        .encode(event, &mut [0; 128], &mut [0; 160])
        .unwrap()
        .to_vec();
    peer.write_all(&wrong).await.unwrap();
    peer.write_all(&wire(event)).await.unwrap();
    peer.write_all(&wire(event)[..7]).await.unwrap();
    drop(peer);
    assert!(matches!(
        rx.receive(|_| ()).await,
        Err(Error::Protocol(
            control_link_tokio::protocol::Error::WrongApplication
        ))
    ));
    rx.receive(|m| assert_eq!(m, event)).await.unwrap();
    assert!(matches!(
        rx.receive(|_| ()).await,
        Err(Error::Framing(framing::Error::Truncated))
    ));
    assert!(matches!(rx.receive(|_| ()).await, Err(Error::Closed)));
}
