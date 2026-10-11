use super::*;
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use embedded_io_async::ErrorKind;
use std::{cell::RefCell, rc::Rc, vec, vec::Vec};

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Pin(PinState),
    Delay(u32),
    Write(Vec<u8>),
    Flush(PinState),
    Read,
}
struct State {
    events: Vec<Event>,
    level: PinState,
    tx_level: PinState,
    queued: Vec<u8>,
    wire: Vec<u8>,
    read: Vec<u8>,
    write_error: bool,
    flush_error: bool,
    read_error: bool,
    zero: bool,
}
type Shared = Rc<RefCell<State>>;
struct Serial(Shared);
struct Pin(Shared);
struct Delay(Shared);
type Bus = Rs485<Serial, Pin, Delay>;

async fn suspend() {
    let mut once = false;
    core::future::poll_fn(|cx| {
        if once {
            Poll::Ready(())
        } else {
            once = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await
}
impl ErrorType for Serial {
    type Error = ErrorKind;
}
impl Write for Serial {
    async fn write(&mut self, bytes: &[u8]) -> Result<usize, ErrorKind> {
        let count = bytes.len().min(2);
        {
            let mut s = self.0.borrow_mut();
            s.events.push(Event::Write(bytes[..count].to_vec()));
            if s.write_error {
                return Err(ErrorKind::Other);
            }
            if s.zero {
                return Ok(0);
            }
            s.queued.extend_from_slice(&bytes[..count]);
        }
        // Deliberately permits queued bytes on cancellation, as the trait does.
        suspend().await;
        Ok(count)
    }
    async fn flush(&mut self) -> Result<(), ErrorKind> {
        suspend().await;
        let mut s = self.0.borrow_mut();
        let level = s.level;
        s.events.push(Event::Flush(level));
        if s.flush_error {
            return Err(ErrorKind::Other);
        }
        let queued = core::mem::take(&mut s.queued);
        if level == s.tx_level {
            s.wire.extend_from_slice(&queued);
        }
        Ok(())
    }
}
impl Read for Serial {
    async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, ErrorKind> {
        suspend().await;
        let mut s = self.0.borrow_mut();
        s.events.push(Event::Read);
        if s.read_error {
            return Err(ErrorKind::Other);
        }
        let count = buffer.len().min(s.read.len()).min(2);
        buffer[..count].copy_from_slice(&s.read[..count]);
        s.read.drain(..count);
        Ok(count)
    }
}
impl embedded_hal::digital::ErrorType for Pin {
    type Error = Infallible;
}
impl OutputPin for Pin {
    fn set_low(&mut self) -> Result<(), Infallible> {
        self.set_state(PinState::Low)
    }
    fn set_high(&mut self) -> Result<(), Infallible> {
        self.set_state(PinState::High)
    }
    fn set_state(&mut self, state: PinState) -> Result<(), Infallible> {
        let mut s = self.0.borrow_mut();
        s.level = state;
        s.events.push(Event::Pin(state));
        Ok(())
    }
}
impl DelayNs for Delay {
    async fn delay_ns(&mut self, ns: u32) {
        self.0.borrow_mut().events.push(Event::Delay(ns));
        suspend().await;
    }
}
fn run<T>(f: impl Future<Output = T>) -> T {
    let mut f = core::pin::pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..100 {
        if let Poll::Ready(value) = f.as_mut().poll(&mut cx) {
            return value;
        }
    }
    panic!("future did not complete");
}
fn setup(tx_level: PinState) -> (Bus, Shared) {
    let s = Rc::new(RefCell::new(State {
        events: vec![],
        level: !tx_level,
        tx_level,
        queued: vec![0xee],
        wire: vec![],
        read: vec![1, 2, 3],
        write_error: false,
        flush_error: false,
        read_error: false,
        zero: false,
    }));
    let mut bus = Rs485::new(
        Serial(s.clone()),
        Pin(s.clone()),
        Delay(s.clone()),
        Config {
            tx_level,
            enable_us: 2,
            hold_us: 3,
            receive_us: 4,
        },
    );
    assert!(bus.recovery_required());
    assert_eq!(run(bus.send(&[1])), Err(Error::RecoveryRequired));
    run(bus.recover()).unwrap();
    assert!(s.borrow().wire.is_empty());
    assert!(s.borrow().queued.is_empty());
    s.borrow_mut().events.clear();
    (bus, s)
}

#[test]
fn short_writes_keep_direction_through_flush_and_hold_for_both_polarities() {
    for tx in [PinState::High, PinState::Low] {
        let (mut bus, s) = setup(tx);
        run(bus.write_all(&[1, 2, 3])).unwrap();
        assert_eq!(s.borrow().wire, [1, 2, 3]);
        assert_eq!(
            s.borrow().events,
            [
                Event::Pin(tx),
                Event::Delay(2000),
                Event::Write(vec![1, 2]),
                Event::Write(vec![3]),
                Event::Flush(tx),
                Event::Delay(3000),
                Event::Pin(!tx),
                Event::Delay(4000)
            ]
        );
        assert!(!bus.recovery_required());
        run(bus.flush()).unwrap();
    }
}

#[test]
fn cancellation_at_every_send_await_releases_bus_and_requires_drain() {
    // enable, write1, write2, flush, hold, receive settling.
    for polls in 1..=6 {
        let (mut bus, s) = setup(PinState::High);
        {
            let mut f = core::pin::pin!(bus.send(&[1, 2, 3]));
            let mut cx = Context::from_waker(Waker::noop());
            for _ in 0..polls {
                assert!(f.as_mut().poll(&mut cx).is_pending());
            }
        }
        assert_eq!(s.borrow().level, PinState::Low);
        assert!(bus.recovery_required());
        assert_eq!(run(bus.send(&[9])), Err(Error::RecoveryRequired));
        assert_eq!(run(bus.flush()), Err(Error::RecoveryRequired));
        let wire_before = s.borrow().wire.clone();
        run(bus.recover()).unwrap();
        assert!(s.borrow().queued.is_empty());
        assert_eq!(
            s.borrow().wire,
            wire_before,
            "recovery must not transmit a suffix"
        );
        run(bus.send(&[9])).unwrap();
        let mut expected = wire_before;
        expected.push(9);
        assert_eq!(s.borrow().wire, expected);
    }
}

#[test]
fn write_and_flush_errors_release_direction() {
    for flush in [false, true] {
        let (mut bus, s) = setup(PinState::High);
        s.borrow_mut().flush_error = flush;
        s.borrow_mut().write_error = !flush;
        assert_eq!(run(bus.send(&[1, 2, 3])), Err(Error::Io(ErrorKind::Other)));
        assert_eq!(s.borrow().level, PinState::Low);
        assert!(bus.recovery_required());
        s.borrow_mut().write_error = false;
        s.borrow_mut().flush_error = false;
        run(bus.recover()).unwrap();
        assert!(s.borrow().wire.is_empty());
    }
}

#[test]
fn failed_or_cancelled_recovery_stays_blocked() {
    let (mut bus, s) = setup(PinState::Low);
    s.borrow_mut().flush_error = true;
    assert_eq!(run(bus.recover()), Err(Error::Io(ErrorKind::Other)));
    s.borrow_mut().flush_error = false;
    for polls in 1..=2 {
        {
            let mut f = core::pin::pin!(bus.recover());
            let mut cx = Context::from_waker(Waker::noop());
            for _ in 0..polls {
                assert!(f.as_mut().poll(&mut cx).is_pending());
            }
        }
        assert!(bus.recovery_required());
        assert_eq!(s.borrow().level, PinState::High);
    }
    run(bus.recover()).unwrap();
}

#[test]
fn empty_operations_do_not_toggle_direction_even_when_blocked() {
    let (mut bus, s) = setup(PinState::High);
    bus.recovery_required = true;
    run(bus.send(&[])).unwrap();
    assert_eq!(run(bus.read(&mut [])), Ok(0));
    assert!(s.borrow().events.is_empty());
}

#[test]
fn read_preserves_short_read_eof_error_and_cancel_semantics() {
    let (mut bus, s) = setup(PinState::High);
    let mut bytes = [0; 8];
    {
        let mut f = core::pin::pin!(bus.read(&mut bytes));
        assert!(
            f.as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    assert_eq!(s.borrow().read, [1, 2, 3]);
    assert_eq!(run(bus.read(&mut bytes)), Ok(2));
    assert_eq!(&bytes[..2], &[1, 2]);
    assert_eq!(run(bus.read(&mut bytes)), Ok(1));
    assert_eq!(run(bus.read(&mut bytes)), Ok(0));
    s.borrow_mut().read_error = true;
    assert_eq!(run(bus.read(&mut bytes)), Err(Error::Io(ErrorKind::Other)));
    assert!(!bus.recovery_required());
    bus.recovery_required = true;
    assert_eq!(run(bus.read(&mut bytes)), Err(Error::RecoveryRequired));
}

#[test]
fn zero_progress_is_an_error_and_drop_releases_pin() {
    let (mut bus, s) = setup(PinState::High);
    s.borrow_mut().zero = true;
    assert_eq!(run(bus.send(&[1])), Err(Error::WriteZero));
    assert!(bus.recovery_required());
    drop(bus);
    assert_eq!(s.borrow().level, PinState::Low);
}
