#![cfg(feature = "supervisor")]
use core::{future::pending, num::NonZeroU64};
use embassy_time::Duration;
use embedded_io_async::{ErrorKind, ErrorType, Read, Write};
use polis_control_link_embassy::{
    Buffers, Error,
    protocol::{ControlLink, Message},
    stream,
    supervisor::{self, Exit, SendError},
};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
const LINK: ControlLink = ControlLink::new(*b"example-app-0001");
fn message(session: u64, body: &[u8]) -> Message<'_> {
    Message::Event {
        session: NonZeroU64::new(session).unwrap(),
        body,
    }
}
fn wire(message: Message<'_>) -> Vec<u8> {
    LINK.encode(message, &mut [0; 128], &mut [0; 160])
        .unwrap()
        .to_vec()
}
#[derive(Default)]
struct Observed {
    rx_drops: usize,
    tx_drops: usize,
    reads: usize,
    writes: usize,
    bytes: Vec<u8>,
}
type Seen = Rc<RefCell<Observed>>;
struct Rx {
    seen: Seen,
    incoming: VecDeque<u8>,
    eof: bool,
}
impl Drop for Rx {
    fn drop(&mut self) {
        self.seen.borrow_mut().rx_drops += 1;
    }
}
impl ErrorType for Rx {
    type Error = ErrorKind;
}
impl Read for Rx {
    async fn read(&mut self, out: &mut [u8]) -> Result<usize, ErrorKind> {
        self.seen.borrow_mut().reads += 1;
        if self.incoming.is_empty() && !self.eof {
            return pending().await;
        }
        let n = out.len().min(self.incoming.len());
        for byte in &mut out[..n] {
            *byte = self.incoming.pop_front().unwrap();
        }
        Ok(n)
    }
}
#[derive(Clone, Copy)]
enum Mode {
    Collect,
    StallAfter(usize),
    Fail,
    Zero,
    StallFlush,
    FailFlush,
}
struct Tx {
    seen: Seen,
    mode: Mode,
}
impl Drop for Tx {
    fn drop(&mut self) {
        self.seen.borrow_mut().tx_drops += 1;
    }
}
impl ErrorType for Tx {
    type Error = ErrorKind;
}
impl Write for Tx {
    async fn write(&mut self, bytes: &[u8]) -> Result<usize, ErrorKind> {
        self.seen.borrow_mut().writes += 1;
        match self.mode {
            Mode::Fail => Err(ErrorKind::BrokenPipe),
            Mode::Zero => Ok(0),
            Mode::StallAfter(limit) => {
                let remaining = limit.saturating_sub(self.seen.borrow().bytes.len());
                if remaining == 0 {
                    return pending().await;
                }
                let n = remaining.min(bytes.len());
                self.seen.borrow_mut().bytes.extend_from_slice(&bytes[..n]);
                Ok(n)
            }
            _ => {
                self.seen.borrow_mut().bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
        }
    }
    async fn flush(&mut self) -> Result<(), ErrorKind> {
        match self.mode {
            Mode::StallFlush => pending().await,
            Mode::FailFlush => Err(ErrorKind::BrokenPipe),
            _ => Ok(()),
        }
    }
}
fn backends(seen: &Seen, incoming: Vec<u8>, eof: bool, mode: Mode) -> (Rx, Tx) {
    (
        Rx {
            seen: seen.clone(),
            incoming: incoming.into(),
            eof,
        },
        Tx {
            seen: seen.clone(),
            mode,
        },
    )
}
fn assert_dropped(seen: &Seen) {
    let s = seen.borrow();
    assert_eq!((s.rx_drops, s.tx_drops), (1, 1));
}

#[tokio::test]
async fn receive_failure_retires_both_even_if_application_ignores_error() {
    for incoming in [vec![], wire(message(1, b"old"))[..8].to_vec()] {
        let seen = Seen::default();
        let (reader, writer) = backends(&seen, incoming, true, Mode::Collect);
        let (mut rx, mut payload, mut tx) = ([0; 160], [0; 128], [0; 160]);
        let exit = run(
            stream::split(
                LINK,
                reader,
                writer,
                Buffers {
                    receive: &mut rx,
                    payload: &mut payload,
                    transmit: &mut tx,
                },
            ),
            Duration::from_secs(1),
            pending(),
            async |mut receiver| {
                assert!(receiver.receive(|_| ()).await.is_err());
                pending().await
            },
            async |mut sender| {
                sender.send(message(1, b"must not send")).await.unwrap();
            },
        )
        .await;
        assert_eq!(exit, Exit::ReceiveFailed);
        assert_eq!(seen.borrow().writes, 0);
        assert_dropped(&seen);
    }
}

#[tokio::test]
async fn send_errors_retire_a_pending_receiver() {
    for mode in [Mode::Fail, Mode::Zero, Mode::FailFlush] {
        let seen = Seen::default();
        let (reader, writer) = backends(&seen, vec![], false, mode);
        let (mut rx, mut payload, mut tx) = ([0; 160], [0; 128], [0; 160]);
        let exit = run(
            stream::split(
                LINK,
                reader,
                writer,
                Buffers {
                    receive: &mut rx,
                    payload: &mut payload,
                    transmit: &mut tx,
                },
            ),
            Duration::from_secs(1),
            pending(),
            async |mut receiver| {
                receiver.receive(|_| ()).await.unwrap();
            },
            async |mut sender| {
                assert!(matches!(
                    sender.send(message(1, b"once")).await,
                    Err(SendError::Transport(_))
                ));
                pending().await
            },
        )
        .await;
        assert_eq!(exit, Exit::SendFailed);
        assert_dropped(&seen);
    }
}

#[tokio::test]
async fn timeout_during_write_or_flush_reconnects_without_replay() {
    for mode in [Mode::StallAfter(5), Mode::StallFlush] {
        let old = Seen::default();
        let (reader, writer) = backends(
            &old,
            wire(message(1, b"partial"))[..8].to_vec(),
            false,
            mode,
        );
        let (mut rx, mut payload, mut tx) = ([0; 160], [0; 128], [0; 160]);
        let exit = run(
            stream::split(
                LINK,
                reader,
                writer,
                Buffers {
                    receive: &mut rx,
                    payload: &mut payload,
                    transmit: &mut tx,
                },
            ),
            Duration::from_millis(2),
            pending(),
            async |mut receiver| {
                receiver.receive(|_| ()).await.unwrap();
            },
            async |mut sender| {
                assert!(matches!(
                    sender.send(message(1, b"never replay")).await,
                    Err(SendError::TimedOut)
                ));
                pending().await
            },
        )
        .await;
        assert_eq!(exit, Exit::SendTimedOut);
        assert_dropped(&old);
        // Reuse the SAME storage. Old partial RX and uncertain TX are discarded.
        let new = Seen::default();
        let (reader, writer) = backends(&new, wire(message(2, b"new peer")), false, Mode::Collect);
        let exit = run(
            stream::split(
                LINK,
                reader,
                writer,
                Buffers {
                    receive: &mut rx,
                    payload: &mut payload,
                    transmit: &mut tx,
                },
            ),
            Duration::from_secs(1),
            pending(),
            async |mut receiver| {
                receiver
                    .receive(|m| assert_eq!(m, message(2, b"new peer")))
                    .await
                    .unwrap();
                pending().await
            },
            async |mut sender| {
                sender.send(message(2, b"fresh command")).await.unwrap();
            },
        )
        .await;
        assert_eq!(exit, Exit::TransmitEnded);
        assert_eq!(new.borrow().bytes, wire(message(2, b"fresh command")));
        assert_dropped(&new);
    }
}

#[tokio::test]
async fn cancelling_one_send_retires_both_even_when_tx_loop_stays_pending() {
    let seen = Seen::default();
    let (reader, writer) = backends(&seen, vec![], false, Mode::StallAfter(4));
    let (mut rx, mut payload, mut tx) = ([0; 160], [0; 128], [0; 160]);
    let exit = run(
        stream::split(
            LINK,
            reader,
            writer,
            Buffers {
                receive: &mut rx,
                payload: &mut payload,
                transmit: &mut tx,
            },
        ),
        Duration::from_secs(10),
        pending(),
        async |mut receiver| {
            receiver.receive(|_| ()).await.unwrap();
        },
        async |mut sender| {
            assert!(
                tokio::time::timeout(
                    std::time::Duration::from_millis(2),
                    sender.send(message(1, b"once"))
                )
                .await
                .is_err()
            );
            assert!(matches!(
                sender.send(message(1, b"must not send")).await,
                Err(SendError::Transport(Error::Interrupted))
            ));
            pending().await
        },
    )
    .await;
    assert_eq!(exit, Exit::SendCancelled);
    assert_eq!(seen.borrow().bytes.len(), 4);
    assert_dropped(&seen);
}

#[tokio::test]
async fn reset_and_outer_cancellation_drop_both_in_flight_halves() {
    for reset in [true, false] {
        let seen = Seen::default();
        let (reader, writer) = backends(
            &seen,
            wire(message(1, b"old"))[..8].to_vec(),
            false,
            Mode::StallAfter(4),
        );
        let (mut rx, mut payload, mut tx) = ([0; 160], [0; 128], [0; 160]);
        let future = run(
            stream::split(
                LINK,
                reader,
                writer,
                Buffers {
                    receive: &mut rx,
                    payload: &mut payload,
                    transmit: &mut tx,
                },
            ),
            Duration::from_secs(10),
            async {
                if reset {
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await
                } else {
                    pending().await
                }
            },
            async |mut receiver| {
                receiver.receive(|_| ()).await.unwrap();
            },
            async |mut sender| {
                sender.send(message(1, b"once")).await.unwrap();
            },
        );
        let result = tokio::time::timeout(std::time::Duration::from_millis(5), future).await;
        if reset {
            assert_eq!(result.unwrap(), Exit::Reset);
        } else {
            assert!(result.is_err());
        }
        assert_dropped(&seen);
        assert_eq!(seen.borrow().bytes.len(), 4);
    }
}

#[tokio::test]
async fn already_pending_reset_prevents_all_io() {
    let seen = Seen::default();
    let (reader, writer) = backends(&seen, wire(message(1, b"stale")), false, Mode::Collect);
    let (mut rx, mut payload, mut tx) = ([0; 160], [0; 128], [0; 160]);
    let exit = run(
        stream::split(
            LINK,
            reader,
            writer,
            Buffers {
                receive: &mut rx,
                payload: &mut payload,
                transmit: &mut tx,
            },
        ),
        Duration::from_secs(1),
        async {},
        async |_| panic!("reset must win before RX runs"),
        async |_| panic!("reset must win before TX runs"),
    )
    .await;
    assert_eq!(exit, Exit::Reset);
    assert_dropped(&seen);
    assert_eq!((seen.borrow().reads, seen.borrow().writes), (0, 0));
}

#[tokio::test]
async fn recoverable_protocol_errors_do_not_end_the_connection() {
    let event = message(1, b"new");
    let mut incoming = ControlLink::new(*b"other---app-0001")
        .encode(event, &mut [0; 128], &mut [0; 160])
        .unwrap()
        .to_vec();
    incoming.extend(wire(event));
    let seen = Seen::default();
    let (reader, writer) = backends(&seen, incoming, false, Mode::Collect);
    let (mut rx, mut payload, mut tx) = ([0; 160], [0; 128], [0; 160]);
    let exit = run(
        stream::split(
            LINK,
            reader,
            writer,
            Buffers {
                receive: &mut rx,
                payload: &mut payload,
                transmit: &mut tx,
            },
        ),
        Duration::from_secs(1),
        pending(),
        async |mut receiver| {
            assert!(matches!(
                receiver.receive(|_| ()).await,
                Err(Error::Protocol(_))
            ));
            receiver.receive(|m| assert_eq!(m, event)).await.unwrap();
        },
        async |_| pending().await,
    )
    .await;
    assert_eq!(exit, Exit::ReceiveEnded);
    assert_dropped(&seen);
}

// Closure-based test scenarios exercise the public handler entry point.
struct Loops<A, B>(A, B);
impl<R: Read, W: Write, A, B> supervisor::ConnectionHandler<R, W> for Loops<A, B>
where
    A: for<'s, 'a> AsyncFn(supervisor::Receiver<'s, 'a, R>),
    B: for<'s, 'a> AsyncFn(supervisor::Sender<'s, 'a, W>),
{
    async fn receive(&self, receiver: supervisor::Receiver<'_, '_, R>) {
        self.0(receiver).await
    }
    async fn transmit(&self, sender: supervisor::Sender<'_, '_, W>) {
        self.1(sender).await
    }
}
async fn run<R: Read, W: Write>(
    pair: stream::Pair<'_, R, W>,
    timeout: Duration,
    reset: impl core::future::Future<Output = ()>,
    receive: impl for<'s, 'a> AsyncFn(supervisor::Receiver<'s, 'a, R>),
    transmit: impl for<'s, 'a> AsyncFn(supervisor::Sender<'s, 'a, W>),
) -> Exit {
    supervisor::run(pair, timeout, reset, &Loops(receive, transmit)).await
}
