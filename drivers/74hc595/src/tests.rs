use super::*;
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use std::{cell::RefCell, rc::Rc, vec, vec::Vec};

#[derive(Clone, Debug, PartialEq)]
enum Call {
    Write(Vec<u8>),
    Flush,
    Latch(bool),
    Delay(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fault(u8);
impl embedded_hal::spi::Error for Fault {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}
impl embedded_hal::digital::Error for Fault {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        embedded_hal::digital::ErrorKind::Other
    }
}

struct State {
    shift: Vec<u8>,
    outputs: Vec<u8>,
    pending: Vec<u8>,
    latch: bool,
    now: u64,
    edge: u64,
    clock: u64,
    calls: Vec<Call>,
    failure: Option<(usize, bool)>,
}
type Shared = Rc<RefCell<State>>;
struct Bus(Shared);
struct Latch(Shared);
struct Delay(Shared);
type Driver<const N: usize> = Hc595<Bus, Latch, Delay, N>;

fn resources(n: usize) -> (Bus, Latch, Delay, Shared) {
    let state = Rc::new(RefCell::new(State {
        shift: vec![0xa5; n],
        outputs: vec![0x5a; n],
        pending: Vec::new(),
        latch: false,
        now: 0,
        edge: 0,
        clock: 0,
        calls: Vec::new(),
        failure: None,
    }));
    (
        Bus(state.clone()),
        Latch(state.clone()),
        Delay(state.clone()),
        state,
    )
}
fn driver<const N: usize>() -> (Driver<N>, Shared) {
    let (bus, latch, delay, state) = resources(N);
    (Hc595::new(bus, latch, delay).unwrap(), state)
}
fn record(state: &Shared, call: Call) -> Option<bool> {
    let mut s = state.borrow_mut();
    let index = s.calls.len();
    s.calls.push(call);
    s.failure
        .filter(|(i, _)| *i == index)
        .map(|(_, after)| after)
}
async fn suspend() {
    let mut first = true;
    core::future::poll_fn(|cx| {
        if first {
            first = false;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await
}
impl embedded_hal::spi::ErrorType for Bus {
    type Error = Fault;
}
impl SpiBus<u8> for Bus {
    async fn write(&mut self, bytes: &[u8]) -> Result<(), Fault> {
        let failure = record(&self.0, Call::Write(bytes.to_vec()));
        suspend().await;
        if failure == Some(false) {
            return Err(Fault(17));
        }
        // A HAL may return with the physical transmission still outstanding.
        self.0.borrow_mut().pending = bytes.to_vec();
        suspend().await;
        if failure == Some(true) {
            return Err(Fault(17));
        }
        Ok(())
    }
    async fn flush(&mut self) -> Result<(), Fault> {
        let failure = record(&self.0, Call::Flush);
        suspend().await;
        if failure == Some(false) {
            return Err(Fault(17));
        }
        {
            let mut s = self.0.borrow_mut();
            let bytes = core::mem::take(&mut s.pending);
            // Model every rising SHCP edge, feeding nearest chip first.
            for byte in bytes {
                for bit in (0..8).rev() {
                    let mut carry = (byte >> bit) & 1;
                    for chip in s.shift.iter_mut().rev() {
                        let next = *chip >> 7;
                        *chip = (*chip << 1) | carry;
                        carry = next;
                    }
                    s.now += 1_000; // 1 MHz example clock.
                    s.clock = s.now;
                }
            }
        }
        suspend().await;
        if failure == Some(true) {
            return Err(Fault(17));
        }
        Ok(())
    }
    async fn read(&mut self, _: &mut [u8]) -> Result<(), Fault> {
        panic!("unexpected read")
    }
    async fn transfer(&mut self, _: &mut [u8], _: &[u8]) -> Result<(), Fault> {
        panic!("unexpected transfer")
    }
    async fn transfer_in_place(&mut self, _: &mut [u8]) -> Result<(), Fault> {
        panic!("unexpected transfer")
    }
}
impl embedded_hal::digital::ErrorType for Latch {
    type Error = Fault;
}
impl OutputPin for Latch {
    fn set_low(&mut self) -> Result<(), Fault> {
        self.set(false)
    }
    fn set_high(&mut self) -> Result<(), Fault> {
        self.set(true)
    }
}
impl Latch {
    fn set(&mut self, high: bool) -> Result<(), Fault> {
        let failure = record(&self.0, Call::Latch(high));
        if failure == Some(false) {
            return Err(Fault(23));
        }
        let mut s = self.0.borrow_mut();
        if high != s.latch {
            assert!(s.now - s.edge >= 1_000, "latch pulse too short");
            if high {
                assert!(
                    s.pending.is_empty(),
                    "latched before physical transfer completed"
                );
                assert!(s.now - s.clock >= 1_000, "missing final clock setup delay");
                s.outputs = s.shift.clone();
            }
            s.latch = high;
            s.edge = s.now;
        }
        if failure == Some(true) {
            return Err(Fault(23));
        }
        Ok(())
    }
}
impl DelayNs for Delay {
    async fn delay_ns(&mut self, ns: u32) {
        record(&self.0, Call::Delay(ns));
        suspend().await; // Drop before time has elapsed.
        self.0.borrow_mut().now += u64::from(ns);
        suspend().await;
    }
}
fn run<T>(future: impl Future<Output = T>) -> T {
    let mut future = core::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..100 {
        if let Poll::Ready(result) = future.as_mut().poll(&mut cx) {
            return result;
        }
    }
    panic!("future failed to finish")
}
fn cancel(future: impl Future, polls: usize) {
    let mut future = core::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..polls {
        assert!(future.as_mut().poll(&mut cx).is_pending());
    }
}

#[test]
fn construction_and_release_do_no_io_and_accept_borrowed_resources() {
    let (mut bus, mut latch, mut delay, state) = resources(1);
    let mut dev = Hc595::<_, _, _, 1>::new(&mut bus, &mut latch, &mut delay).unwrap();
    let _ = dev.bus_mut();
    let _ = dev.into_inner();
    assert!(state.borrow().calls.is_empty());
    let (bus, latch, delay, state) = resources(0);
    assert!(matches!(
        Hc595::<_, _, _, 0>::new(bus, latch, delay),
        Err(Error::EmptyChain)
    ));
    assert!(state.borrow().calls.is_empty());
}

#[test]
fn every_single_chip_value_reaches_the_correct_outputs() {
    let (mut dev, state) = driver::<1>();
    for value in 0..=u8::MAX {
        run(dev.write_outputs(&[value])).unwrap();
        let s = state.borrow();
        assert_eq!(s.outputs, [value]);
        assert!(!s.latch);
        assert!(s.pending.is_empty());
    }
}

#[test]
fn chain_bytes_are_farthest_first_and_latch_follows_flush() {
    let (mut dev, state) = driver::<3>();
    run(dev.write_outputs(&[0x81, 0x42, 0x24])).unwrap();
    assert_eq!(state.borrow().outputs, [0x81, 0x42, 0x24]);
    assert_eq!(
        state.borrow().calls,
        [
            Call::Delay(1_000),
            Call::Latch(false),
            Call::Delay(1_000),
            Call::Write(vec![0x81, 0x42, 0x24]),
            Call::Flush,
            Call::Delay(1_000),
            Call::Latch(true),
            Call::Delay(1_000),
            Call::Latch(false),
        ]
    );
}

#[test]
fn clear_updates_storage_and_repeated_frames_are_always_sent() {
    let (mut dev, state) = driver::<2>();
    for _ in 0..2 {
        run(dev.clear()).unwrap();
        assert_eq!(state.borrow().outputs, [0, 0]);
        // Simulate an external reset/corruption between calls.
        state.borrow_mut().shift.fill(0xff);
        state.borrow_mut().outputs.fill(0xff);
    }
    assert_eq!(
        state
            .borrow()
            .calls
            .iter()
            .filter(|c| matches!(c, Call::Write(_)))
            .count(),
        2
    );
}

#[test]
fn failures_preserve_errors_stop_and_allow_explicit_recovery() {
    // Every fallible step, with errors before AND after possible hardware effects.
    for index in [1, 3, 4, 6, 8] {
        for after in [false, true] {
            let (mut dev, state) = driver::<2>();
            state.borrow_mut().failure = Some((index, after));
            let expected = if index == 3 || index == 4 {
                Error::Spi(Fault(17))
            } else {
                Error::Latch(Fault(23))
            };
            assert_eq!(run(dev.write_outputs(&[0x12, 0x34])), Err(expected));
            assert_eq!(state.borrow().calls.len(), index + 1);
            assert_eq!(
                state.borrow().outputs,
                if index > 6 || (index == 6 && after) {
                    [0x12, 0x34]
                } else {
                    [0x5a; 2]
                }
            );
            // Caller performs HAL-specific recovery; incomplete queued transfer discarded.
            state.borrow_mut().pending.clear();
            state.borrow_mut().failure = None;
            run(dev.write_outputs(&[0xab, 0xcd])).unwrap();
            assert_eq!(state.borrow().outputs, [0xab, 0xcd]);
            assert!(!state.borrow().latch);
        }
    }
}

#[test]
fn cancellation_at_every_await_preserves_storage_until_latch_and_recovers() {
    // Four delays plus write and flush, each suspending before and after effects.
    for polls in 1..=12 {
        let (mut dev, state) = driver::<2>();
        cancel(dev.write_outputs(&[0x12, 0x34]), polls);
        assert_eq!(
            state.borrow().outputs,
            if polls >= 11 { [0x12, 0x34] } else { [0x5a; 2] }
        );
        assert_eq!(state.borrow().latch, polls >= 11);
        // No time advances here: the next write must finish a cancelled high pulse.
        state.borrow_mut().pending.clear();
        run(dev.write_outputs(&[0xab, 0xcd])).unwrap();
        assert_eq!(state.borrow().outputs, [0xab, 0xcd]);
        assert!(!state.borrow().latch);
    }
}
