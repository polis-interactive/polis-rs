use core::{convert::Infallible, future::Future, num::NonZeroU64, task::Context};
use embedded_io_async::{ErrorKind, ErrorType, Read, Write};
use polis_control_link_embassy::{
    Buffers, Error,
    half_duplex::Link,
    protocol::{ControlLink, Incoming, Message, RequestGate, Requests, StateError},
};
use std::{cell::RefCell, collections::VecDeque, rc::Rc, task::Waker};

const CODEC: ControlLink = ControlLink::new(*b"example-app-0001");
fn session() -> NonZeroU64 {
    NonZeroU64::new(23).unwrap()
}
fn event() -> Message<'static> {
    Message::Event {
        session: session(),
        body: b"status",
    }
}
fn wire(message: Message<'_>) -> Vec<u8> {
    CODEC
        .encode(message, &mut [0; 128], &mut [0; 160])
        .unwrap()
        .to_vec()
}
struct Storage {
    receive: [u8; 160],
    payload: [u8; 128],
    transmit: [u8; 160],
}
impl Default for Storage {
    fn default() -> Self {
        Self {
            receive: [0; 160],
            payload: [0; 128],
            transmit: [0; 160],
        }
    }
}
impl Storage {
    fn link<S: Read + Write>(&mut self, io: S) -> Link<'_, S> {
        Link::new(
            io,
            CODEC,
            Buffers {
                receive: &mut self.receive,
                payload: &mut self.payload,
                transmit: &mut self.transmit,
            },
        )
    }
}

// Poll exactly once and drop a pending operation, with no wall-clock sleeps.
fn cancel(future: impl Future) {
    let mut future = core::pin::pin!(future);
    assert!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
}

struct State {
    input: VecDeque<u8>,
    output: Vec<u8>,
    read_limit: usize,
    write_limit: usize,
    eof: bool,
    read_error: bool,
    write_error: bool,
    flush_error: bool,
    zero_write: bool,
    stage: usize,
    block_at: Option<usize>,
    de: bool,
    trace: Vec<&'static str>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            input: VecDeque::new(),
            output: Vec::new(),
            read_limit: usize::MAX,
            write_limit: usize::MAX,
            eof: false,
            read_error: false,
            write_error: false,
            flush_error: false,
            zero_write: false,
            stage: 0,
            block_at: None,
            de: false,
            trace: Vec::new(),
        }
    }
}
type Shared = Rc<RefCell<State>>;
async fn checkpoint(state: &Shared, name: &'static str) {
    let block = {
        let mut state = state.borrow_mut();
        state.trace.push(name);
        state.stage += 1;
        state.block_at == Some(state.stage)
    };
    if block {
        core::future::pending::<()>().await;
    }
}
struct Memory(Shared);
impl ErrorType for Memory {
    type Error = ErrorKind;
}
impl Read for Memory {
    async fn read(&mut self, bytes: &mut [u8]) -> Result<usize, ErrorKind> {
        let wait = {
            let state = self.0.borrow();
            state.input.is_empty() && !state.eof && !state.read_error
        };
        if wait {
            core::future::pending::<()>().await;
        }
        let mut state = self.0.borrow_mut();
        if state.read_error {
            return Err(ErrorKind::Other);
        }
        let n = bytes.len().min(state.read_limit).min(state.input.len());
        for byte in &mut bytes[..n] {
            *byte = state.input.pop_front().unwrap();
        }
        Ok(n)
    }
}
impl Write for Memory {
    async fn write(&mut self, bytes: &[u8]) -> Result<usize, ErrorKind> {
        checkpoint(&self.0, "write").await;
        let mut state = self.0.borrow_mut();
        if state.write_error {
            return Err(ErrorKind::Other);
        }
        if state.zero_write {
            return Ok(0);
        }
        let n = bytes.len().min(state.write_limit);
        state.output.extend_from_slice(&bytes[..n]);
        Ok(n)
    }
    async fn flush(&mut self) -> Result<(), ErrorKind> {
        checkpoint(&self.0, "flush").await;
        if self.0.borrow().flush_error {
            Err(ErrorKind::Other)
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn short_io_matches_existing_wire_and_keeps_coalesced_frames_across_sends() {
    for read_limit in [1, 256] {
        let shared = Shared::default();
        shared.borrow_mut().read_limit = read_limit;
        shared.borrow_mut().write_limit = 3;
        let mut storage = Storage::default();
        let mut link = storage.link(Memory(shared.clone()));
        let mut requests = Requests::<1>::new(session());
        let hello = requests.reserve().unwrap();
        let response = Message::Response {
            key: hello,
            body: b"identity",
        };
        shared.borrow_mut().input.extend(wire(response));
        shared.borrow_mut().input.extend(wire(event()));
        link.receive(|m| {
            assert_eq!(
                requests.receive(m),
                Ok(Incoming::Response {
                    key: hello,
                    body: b"identity"
                })
            )
        })
        .await
        .unwrap();
        let key = requests.reserve().unwrap();
        let request = Message::Request {
            key,
            body: b"command",
        };
        link.send(request).await.unwrap();
        assert_eq!(shared.borrow().output, wire(request));
        link.receive(|m| assert_eq!(requests.receive(m), Ok(Incoming::Event { body: b"status" })))
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn cancelling_receive_retains_partial_frame_and_allows_transmit() {
    let shared = Shared::default();
    let frame = wire(event());
    shared.borrow_mut().input.extend(&frame[..7]);
    let mut storage = Storage::default();
    let mut link = storage.link(Memory(shared.clone()));
    cancel(link.receive(|_| ()));
    // Application has explicitly granted this transmit turn after the read ended.
    link.send(event()).await.unwrap();
    shared.borrow_mut().input.extend(&frame[7..]);
    link.receive(|m| assert_eq!(m, event())).await.unwrap();
}

#[tokio::test]
async fn malformed_foreign_and_truncated_frames_resynchronize() {
    let shared = Shared::default();
    let frame = wire(event());
    let foreign = ControlLink::new(*b"other---app-0001")
        .encode(event(), &mut [0; 128], &mut [0; 160])
        .unwrap()
        .to_vec();
    shared.borrow_mut().input.extend([0, 2, 0]); // Malformed COBS packet.
    shared.borrow_mut().input.extend(&frame[..7]);
    shared.borrow_mut().input.extend(foreign); // Leading delimiter ends truncated packet.
    shared.borrow_mut().input.extend(&frame);
    let mut storage = Storage::default();
    let mut link = storage.link(Memory(shared));
    assert!(matches!(link.receive(|_| ()).await, Err(Error::Framing(_))));
    assert!(matches!(link.receive(|_| ()).await, Err(Error::Framing(_))));
    assert!(matches!(
        link.receive(|_| ()).await,
        Err(Error::Protocol(_))
    ));
    link.receive(|m| assert_eq!(m, event())).await.unwrap();
    link.send(event()).await.unwrap();
}

#[tokio::test]
async fn receive_error_or_eof_retires_both_directions() {
    for (read_error, partial) in [(true, false), (false, false), (false, true)] {
        let shared = Shared::default();
        shared.borrow_mut().read_error = read_error;
        shared.borrow_mut().eof = true;
        if partial {
            shared.borrow_mut().input.extend(&wire(event())[..7]);
        }
        let mut storage = Storage::default();
        let mut link = storage.link(Memory(shared.clone()));
        let error = link.receive(|_| ()).await.unwrap_err();
        match (read_error, partial) {
            (true, _) => assert!(matches!(error, Error::Io(_))),
            (_, true) => assert!(matches!(
                error,
                Error::Framing(polis_framing::Error::Truncated)
            )),
            _ => assert!(matches!(error, Error::Closed)),
        }
        assert!(matches!(link.send(event()).await, Err(Error::Closed)));
        assert!(matches!(link.receive(|_| ()).await, Err(Error::Closed)));
        assert!(shared.borrow().output.is_empty());
    }
}

#[tokio::test]
async fn encoding_failure_does_no_io_and_is_retryable() {
    let shared = Shared::default();
    shared.borrow_mut().input.extend(wire(event()));
    let mut storage = Storage::default();
    let mut link = storage.link(Memory(shared.clone()));
    assert!(matches!(
        link.send(Message::Event {
            session: session(),
            body: &[1; 129]
        })
        .await,
        Err(Error::Protocol(_))
    ));
    assert!(shared.borrow().trace.is_empty());
    link.send(event()).await.unwrap();
    link.receive(|m| assert_eq!(m, event())).await.unwrap();
}

#[tokio::test]
async fn write_zero_write_error_and_flush_error_retire_both_directions() {
    for fault in 0..3 {
        let shared = Shared::default();
        shared.borrow_mut().zero_write = fault == 0;
        shared.borrow_mut().write_error = fault == 1;
        shared.borrow_mut().flush_error = fault == 2;
        let mut storage = Storage::default();
        let mut link = storage.link(Memory(shared.clone()));
        let error = link.send(event()).await.unwrap_err();
        if fault == 0 {
            assert!(matches!(error, Error::WriteZero));
        } else {
            assert!(matches!(error, Error::Io(_)));
        }
        let trace = shared.borrow().trace.clone();
        assert!(matches!(link.send(event()).await, Err(Error::Interrupted)));
        assert!(matches!(
            link.receive(|_| ()).await,
            Err(Error::Interrupted)
        ));
        assert_eq!(shared.borrow().trace, trace);
    }
}

struct Direction(Shared);
impl embedded_hal::digital::ErrorType for Direction {
    type Error = Infallible;
}
impl embedded_hal::digital::OutputPin for Direction {
    fn set_low(&mut self) -> Result<(), Infallible> {
        let mut state = self.0.borrow_mut();
        state.de = false;
        state.trace.push("rx");
        Ok(())
    }
    fn set_high(&mut self) -> Result<(), Infallible> {
        let mut state = self.0.borrow_mut();
        state.de = true;
        state.trace.push("tx");
        Ok(())
    }
}
struct Delay(Shared);
impl embedded_hal_async::delay::DelayNs for Delay {
    async fn delay_ns(&mut self, _: u32) {
        checkpoint(&self.0, "delay").await;
    }
}
fn bus(state: &Shared) -> polis_rs485::Rs485<Memory, Direction, Delay> {
    polis_rs485::Rs485::new(
        Memory(state.clone()),
        Direction(state.clone()),
        Delay(state.clone()),
        polis_rs485::Config {
            tx_level: embedded_hal::digital::PinState::High,
            enable_us: 12,
            hold_us: 1,
            receive_us: 12,
        },
    )
}

#[tokio::test]
async fn rs485_holds_direction_for_the_entire_frame_and_returns_in_receive_mode() {
    let shared = Shared::default();
    shared.borrow_mut().write_limit = wire(event()).len().div_ceil(2);
    let mut bus = bus(&shared);
    bus.recover().await.unwrap();
    shared.borrow_mut().trace.clear();
    let mut storage = Storage::default();
    let mut link = storage.link(bus);
    link.send(event()).await.unwrap();
    assert_eq!(shared.borrow().output, wire(event()));
    assert_eq!(
        shared.borrow().trace,
        [
            "tx", "delay", "write", "write", "flush", "delay", "rx", "delay"
        ]
    );
    assert!(!shared.borrow().de);
    assert!(!link.into_inner().recovery_required());
}

#[tokio::test]
async fn two_rs485_endpoints_exchange_handshake_then_commands_using_the_same_links() {
    let controller = Shared::default();
    let responder = Shared::default();
    controller.borrow_mut().write_limit = 3;
    responder.borrow_mut().write_limit = 5;
    controller.borrow_mut().read_limit = 2;
    responder.borrow_mut().read_limit = 1;
    let mut a = bus(&controller);
    let mut b = bus(&responder);
    a.recover().await.unwrap();
    b.recover().await.unwrap();
    let (mut storage_a, mut storage_b) = (Storage::default(), Storage::default());
    let (mut a, mut b) = (storage_a.link(a), storage_b.link(b));
    let mut requests = Requests::<1>::new(session());
    let mut gate = RequestGate::new(session());
    for body in [b"hello".as_slice(), b"command", b"status"] {
        let key = requests.reserve().unwrap();
        a.send(Message::Request { key, body }).await.unwrap();
        assert!(!controller.borrow().de);
        responder
            .borrow_mut()
            .input
            .extend(controller.borrow_mut().output.drain(..));
        let received = b
            .receive(|message| {
                let Message::Request {
                    key,
                    body: received,
                } = message
                else {
                    panic!()
                };
                assert_eq!(received, body);
                gate.accept(key).unwrap();
                key
            })
            .await
            .unwrap();
        b.send(Message::Response {
            key: received,
            body,
        })
        .await
        .unwrap();
        assert!(!responder.borrow().de);
        controller
            .borrow_mut()
            .input
            .extend(responder.borrow_mut().output.drain(..));
        a.receive(|message| {
            assert_eq!(
                requests.receive(message),
                Ok(Incoming::Response { key, body })
            )
        })
        .await
        .unwrap();
        assert_eq!(requests.pending_count(), 0);
    }
}

#[tokio::test]
async fn cancelling_each_rs485_send_stage_releases_de_and_requires_explicit_recovery() {
    // Enable, two short writes, physical flush, hold, and receive settling.
    for stage in 1..=6 {
        let shared = Shared::default();
        shared.borrow_mut().write_limit = wire(event()).len().div_ceil(2);
        let mut bus = bus(&shared);
        bus.recover().await.unwrap();
        shared.borrow_mut().stage = 0;
        shared.borrow_mut().block_at = Some(stage);
        let mut storage = Storage::default();
        let mut link = storage.link(bus);
        cancel(link.send(event()));
        assert!(!shared.borrow().de, "stage {stage}");
        assert!(matches!(link.send(event()).await, Err(Error::Interrupted)));
        assert!(matches!(
            link.receive(|_| ()).await,
            Err(Error::Interrupted)
        ));
        let mut bus = link.into_inner();
        assert!(bus.recovery_required());
        let partial = shared.borrow().output.clone();
        // Recovery itself can be cancelled at flush or receive settling.
        for recovery_stage in 1..=2 {
            shared.borrow_mut().stage = 0;
            shared.borrow_mut().block_at = Some(recovery_stage);
            cancel(bus.recover());
            assert!(bus.recovery_required());
            assert!(!shared.borrow().de);
        }
        shared.borrow_mut().block_at = None;
        bus.recover().await.unwrap();
        assert_eq!(
            shared.borrow().output,
            partial,
            "recovery never replays a suffix"
        );
        // The application replaces its trackers/session after physical recovery.
        let mut link = storage.link(bus);
        let next = Message::Event {
            session: NonZeroU64::new(24).unwrap(),
            body: b"fresh",
        };
        link.send(next).await.unwrap();
        assert_eq!(&shared.borrow().output[partial.len()..], wire(next));
    }
}

#[tokio::test]
async fn duplicate_stale_requests_and_lost_responses_remain_application_owned() {
    let shared = Shared::default();
    let mut storage = Storage::default();
    let mut link = storage.link(Memory(shared.clone()));
    let mut requests = Requests::<1>::new(session());
    let key = requests.reserve().unwrap();
    let request = Message::Request { key, body: b"move" };
    let stale = Message::Request {
        key: polis_control_link_embassy::protocol::Key {
            session: NonZeroU64::new(22).unwrap(),
            ..key
        },
        body: b"move",
    };
    for message in [request, request, stale] {
        shared.borrow_mut().input.extend(wire(message));
    }
    let mut gate = RequestGate::new(session());
    for expected in [
        Ok(()),
        Err(StateError::DuplicateRequest),
        Err(StateError::WrongSession),
    ] {
        link.receive(|message| {
            let Message::Request { key, .. } = message else {
                panic!()
            };
            assert_eq!(gate.accept(key), expected);
        })
        .await
        .unwrap();
    }
    link.send(request).await.unwrap();
    cancel(link.receive(|_| ()));
    requests.cancel(key).unwrap(); // Application's response deadline expired.
    shared
        .borrow_mut()
        .input
        .extend(wire(Message::Response { key, body: b"late" }));
    link.receive(|message| assert_eq!(requests.receive(message), Err(StateError::UnknownRequest)))
        .await
        .unwrap();
    assert_eq!(shared.borrow().output, wire(request)); // Exactly one send, no retry.
}
