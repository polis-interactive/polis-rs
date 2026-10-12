use polis_control_link_embassy::{
    Error,
    protocol::{ControlLink, Incoming, Message, RequestGate, Requests},
    stream::{Receiver, Sender},
};
use std::{num::NonZeroU64, time::Duration};
use tokio::io::AsyncWriteExt;
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
            Err(polis_control_link_embassy::protocol::StateError::DuplicateRequest)
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
            polis_control_link_embassy::protocol::Error::WrongApplication
        ))
    ));
    rx.receive(|m| assert_eq!(m, event)).await.unwrap();
    assert!(matches!(
        rx.receive(|_| ()).await,
        Err(Error::Framing(polis_framing::Error::Truncated))
    ));
    assert!(matches!(rx.receive(|_| ()).await, Err(Error::Closed)));
}

struct Stream(tokio::io::DuplexStream);
fn duplex(n: usize) -> (Stream, Stream) {
    let (a, b) = tokio::io::duplex(n);
    (Stream(a), Stream(b))
}
impl core::ops::Deref for Stream {
    type Target = tokio::io::DuplexStream;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl core::ops::DerefMut for Stream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl embedded_io_async::ErrorType for Stream {
    type Error = embedded_io_async::ErrorKind;
}
impl embedded_io_async::Read for Stream {
    async fn read(&mut self, out: &mut [u8]) -> Result<usize, Self::Error> {
        tokio::io::AsyncReadExt::read(&mut self.0, out)
            .await
            .map_err(|_| embedded_io_async::ErrorKind::Other)
    }
}
impl embedded_io_async::Write for Stream {
    async fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
        tokio::io::AsyncWriteExt::write(&mut self.0, bytes)
            .await
            .map_err(|_| embedded_io_async::ErrorKind::Other)
    }
    async fn flush(&mut self) -> Result<(), Self::Error> {
        tokio::io::AsyncWriteExt::flush(&mut self.0)
            .await
            .map_err(|_| embedded_io_async::ErrorKind::Other)
    }
}

struct BrokenWriter {
    zero: bool,
}
impl embedded_io_async::ErrorType for BrokenWriter {
    type Error = embedded_io_async::ErrorKind;
}
impl embedded_io_async::Write for BrokenWriter {
    async fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
        Ok(if self.zero { 0 } else { bytes.len().min(3) })
    }
    async fn flush(&mut self) -> Result<(), Self::Error> {
        Err(embedded_io_async::ErrorKind::Other)
    }
}
#[tokio::test]
async fn zero_write_and_flush_failure_retire_sender() {
    let event = Message::Event {
        session: session(),
        body: b"event",
    };
    for zero in [true, false] {
        let (mut payload, mut wire) = ([0; 128], [0; 160]);
        let mut tx = Sender::new(LINK, BrokenWriter { zero }, &mut payload, &mut wire);
        let err = tx.send(event).await.unwrap_err();
        if zero {
            assert!(matches!(err, Error::WriteZero));
        } else {
            assert!(matches!(err, Error::Io(_)));
        }
        assert!(matches!(tx.send(event).await, Err(Error::Interrupted)));
    }
}
