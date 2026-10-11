#![cfg(any(feature = "tcp", all(unix, feature = "serial")))]
use control_link_tokio::{
    Buffers,
    protocol::{ControlLink, Message, Requests},
    stream::{Receiver, Sender},
};
use std::{num::NonZeroU64, time::Duration};
use tokio::io::{AsyncRead, AsyncWrite};
const LINK: ControlLink = ControlLink::new(*b"example-app-0001");

async fn exchange<
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    R2: AsyncRead + Unpin,
    W2: AsyncWrite + Unpin,
>(
    mut tx: Sender<'_, W>,
    mut rx: Receiver<'_, R>,
    mut peer_tx: Sender<'_, W2>,
    mut peer_rx: Receiver<'_, R2>,
) {
    tokio::time::timeout(Duration::from_secs(2), async {
        let session = NonZeroU64::new(1).unwrap();
        let mut pending = Requests::<2>::new(session);
        let key = pending.reserve().unwrap();
        let mut peer_pending = Requests::<2>::new(session);
        let peer_key = peer_pending.reserve().unwrap();
        // Equal numeric keys are valid in opposite directions.
        assert_eq!(key, peer_key);
        let event = Message::Event {
            session,
            body: b"unsolicited",
        };
        // Both peers send before either receives: application client/server labels
        // do not restrict which peer can initiate traffic.
        let (a, b) = tokio::join!(
            tx.send(Message::Request {
                key,
                body: b"identity"
            }),
            peer_tx.send(Message::Request {
                key: peer_key,
                body: b"host identity"
            })
        );
        a.unwrap();
        b.unwrap();
        peer_rx
            .receive(|m| {
                assert_eq!(
                    m,
                    Message::Request {
                        key,
                        body: b"identity"
                    }
                )
            })
            .await
            .unwrap();
        rx.receive(|m| {
            assert_eq!(
                m,
                Message::Request {
                    key: peer_key,
                    body: b"host identity"
                }
            )
        })
        .await
        .unwrap();
        tx.send(Message::Response {
            key: peer_key,
            body: b"host",
        })
        .await
        .unwrap();
        peer_rx
            .receive(|m| {
                peer_pending.receive(m).unwrap();
            })
            .await
            .unwrap();
        assert_eq!(peer_pending.pending_count(), 0);
        assert_eq!(pending.pending_count(), 1);
        peer_tx.send(event).await.unwrap();
        rx.receive(|m| {
            assert_eq!(m, event);
            pending.receive(m).unwrap();
        })
        .await
        .unwrap();
        peer_tx
            .send(Message::Response {
                key,
                body: b"device",
            })
            .await
            .unwrap();
        rx.receive(|m| {
            pending.receive(m).unwrap();
        })
        .await
        .unwrap();
        assert_eq!(pending.pending_count(), 0);
    })
    .await
    .unwrap();
}

#[cfg(feature = "tcp")]
#[tokio::test]
async fn connected_and_accepted_tcp_are_bidirectional() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socket = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (peer, _) = listener.accept().await.unwrap();
    let (mut rx, mut payload, mut tx, mut prx, mut ppayload, mut ptx) =
        ([0; 160], [0; 128], [0; 160], [0; 160], [0; 128], [0; 160]);
    let (sender, receiver) = control_link_tokio::tcp::split(
        socket,
        LINK,
        Buffers {
            receive: &mut rx,
            payload: &mut payload,
            transmit: &mut tx,
        },
    )
    .unwrap();
    let (peer_sender, peer_receiver) = control_link_tokio::tcp::split(
        peer,
        LINK,
        Buffers {
            receive: &mut prx,
            payload: &mut ppayload,
            transmit: &mut ptx,
        },
    )
    .unwrap();
    exchange(sender, receiver, peer_sender, peer_receiver).await;
}

#[cfg(all(unix, feature = "serial"))]
#[tokio::test]
async fn host_serial_port_opens_and_exchanges_frames() {
    use tokio_serial::SerialPort;
    let (peer, mut port) = tokio_serial::SerialStream::pair().unwrap();
    port.set_exclusive(false).unwrap();
    let name = port.name().unwrap();
    drop(port);
    let builder = tokio_serial::new(name, 115_200);
    let (mut rx, mut payload, mut tx, mut prx, mut ppayload, mut ptx) =
        ([0; 160], [0; 128], [0; 160], [0; 160], [0; 128], [0; 160]);
    let (sender, receiver) = control_link_tokio::serial::open(
        &builder,
        LINK,
        Buffers {
            receive: &mut rx,
            payload: &mut payload,
            transmit: &mut tx,
        },
    )
    .unwrap();
    let (peer_sender, peer_receiver) = control_link_tokio::serial::split(
        peer,
        LINK,
        Buffers {
            receive: &mut prx,
            payload: &mut ppayload,
            transmit: &mut ptx,
        },
    );
    exchange(sender, receiver, peer_sender, peer_receiver).await;
}
