use super::*;
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use embedded_hal_async::i2c::{ErrorKind, ErrorType, NoAcknowledgeSource, Operation};
use std::{cell::RefCell, rc::Rc, vec::Vec};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Call {
    Read(u8, u8),
    Write(u8, u8, u8),
}

struct State {
    registers: [u8; 4],
    calls: Vec<Call>,
    // Zero-based transaction, whether to fail after the hardware effect, error.
    failure: Option<(usize, bool, ErrorKind)>,
}

struct Bus(Rc<RefCell<State>>);

fn device(registers: [u8; 4]) -> (Xca9534<Bus>, Rc<RefCell<State>>) {
    let state = Rc::new(RefCell::new(State {
        registers,
        calls: Vec::new(),
        failure: None,
    }));
    (Xca9534::new(Bus(state.clone()), Address::DEFAULT), state)
}

async fn suspend() {
    let mut pending = true;
    core::future::poll_fn(|cx| {
        if pending {
            pending = false;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;
}

impl ErrorType for Bus {
    type Error = ErrorKind;
}

impl I2c for Bus {
    async fn transaction(
        &mut self,
        address: u8,
        ops: &mut [Operation<'_>],
    ) -> Result<(), ErrorKind> {
        let call = match ops {
            [Operation::Write(command), Operation::Read(data)] => {
                assert_eq!(
                    command.len(),
                    1,
                    "one register pointer before repeated START"
                );
                assert_eq!(data.len(), 1, "single register read");
                assert!(command[0] < 4);
                Call::Read(address, command[0])
            }
            [Operation::Write(data)] => {
                assert_eq!(data.len(), 2, "register and value in one transaction");
                assert!((1..=3).contains(&data[0]), "never write the input register");
                Call::Write(address, data[0], data[1])
            }
            _ => panic!("unexpected operation boundaries"),
        };
        let failure = {
            let mut state = self.0.borrow_mut();
            let index = state.calls.len();
            state.calls.push(call);
            state.failure.filter(|(i, _, _)| *i == index)
        };
        suspend().await; // Cancellation before a possible hardware effect.
        if let Some((_, false, error)) = failure {
            return Err(error);
        }
        match call {
            Call::Read(_, reg) => {
                let Operation::Read(data) = &mut ops[1] else {
                    unreachable!()
                };
                // May fill the caller's buffer before returning an error.
                data[0] = self.0.borrow().registers[reg as usize];
            }
            Call::Write(_, reg, value) => self.0.borrow_mut().registers[reg as usize] = value,
        }
        suspend().await; // Cancellation after hardware changed but before success.
        if let Some((_, true, error)) = failure {
            return Err(error);
        }
        Ok(())
    }
}

fn run<T>(future: impl Future<Output = T>) -> T {
    let mut future = core::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..32 {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
    panic!("future did not finish");
}

fn cancel_after(future: impl Future, polls: usize) {
    let mut future = core::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..polls {
        assert!(future.as_mut().poll(&mut cx).is_pending());
    }
}

#[test]
fn addresses_validate_both_variants_and_all_straps() {
    for value in 0..=u8::MAX {
        assert_eq!(
            Address::new(value).map(Address::as_u8),
            if (0x20..=0x27).contains(&value) || (0x38..=0x3f).contains(&value) {
                Some(value)
            } else {
                None
            }
        );
    }
    for (variant, base) in [(Variant::Standard, 0x20), (Variant::A, 0x38)] {
        for straps in 0..8 {
            let address =
                Address::from_pins(variant, straps & 4 != 0, straps & 2 != 0, straps & 1 != 0);
            assert_eq!(address.as_u8(), base + straps);
            let (dev, state) = device([0x5a, 0xff, 0, 0xff]);
            let mut dev = Xca9534::new(dev.into_inner(), address);
            assert_eq!(run(dev.read_inputs()), Ok(0x5a));
            run(dev.write_outputs(0x12)).unwrap();
            assert_eq!(
                state.borrow().calls,
                [
                    Call::Read(base + straps, 0),
                    Call::Write(base + straps, 1, 0x12)
                ]
            );
        }
    }
    assert_eq!(Address::DEFAULT.as_u8(), 0x20);
    assert_eq!(Address::A_DEFAULT.as_u8(), 0x38);
}

#[test]
fn pins_reject_out_of_range_numbers() {
    for value in 0..=u8::MAX {
        assert_eq!(
            Pin::new(value).map(Pin::mask),
            if value < 8 { Some(1 << value) } else { None }
        );
    }
}

#[test]
fn construction_bus_access_and_release_do_not_touch_hardware() {
    let (dev, state) = device([0x21, 0x43, 0x65, 0x87]);
    let mut bus = dev.into_inner();
    let mut borrowed = Xca9534::new(&mut bus, Address::A_DEFAULT);
    assert_eq!(borrowed.address(), Address::A_DEFAULT);
    let _ = borrowed.bus_mut();
    let _ = borrowed.into_inner();
    assert!(state.borrow().calls.is_empty());
    assert_eq!(state.borrow().registers, [0x21, 0x43, 0x65, 0x87]);
}

#[test]
fn every_read_reselects_its_register_and_uses_repeated_start() {
    let (mut dev, state) = device([0x12, 0x34, 0x56, 0x78]);
    for _ in 0..2 {
        assert_eq!(run(dev.read_configuration()), Ok(0x78));
        assert_eq!(run(dev.read_inputs()), Ok(0x12));
        assert_eq!(run(dev.read_outputs()), Ok(0x34));
        assert_eq!(run(dev.read_polarity()), Ok(0x56));
    }
    assert_eq!(
        state.borrow().calls,
        [
            Call::Read(0x20, 3),
            Call::Read(0x20, 0),
            Call::Read(0x20, 1),
            Call::Read(0x20, 2)
        ]
        .repeat(2)
    );
}

#[test]
fn full_port_writes_preserve_other_registers() {
    for value in 0..=u8::MAX {
        let (mut dev, state) = device([0x55, 0xff, 0, 0xff]);
        run(dev.write_outputs(value)).unwrap();
        assert_eq!(state.borrow().registers, [0x55, value, 0, 0xff]);
        run(dev.write_polarity(value)).unwrap();
        assert_eq!(state.borrow().registers, [0x55, value, value, 0xff]);
        run(dev.write_configuration(value)).unwrap();
        assert_eq!(state.borrow().registers, [0x55, value, value, value]);
        assert_eq!(
            state.borrow().calls,
            [
                Call::Write(0x20, 1, value),
                Call::Write(0x20, 2, value),
                Call::Write(0x20, 3, value)
            ]
        );
    }
}

#[test]
fn initialization_releases_outputs_before_loading_latches_and_enabling() {
    let (mut dev, state) = device([0x5a, 0xff, 0xff, 0]);
    let config = Config {
        outputs: 0x21,
        polarity: 0x40,
        inputs: 0x9e,
    };
    run(dev.init(config)).unwrap();
    assert_eq!(
        state.borrow().calls,
        [
            Call::Write(0x20, 3, 0xff),
            Call::Write(0x20, 1, 0x21),
            Call::Write(0x20, 2, 0x40),
            Call::Write(0x20, 3, 0x9e)
        ]
    );
    assert_eq!(state.borrow().registers, [0x5a, 0x21, 0x40, 0x9e]);
    run(dev.init(Config::default())).unwrap();
    assert_eq!(state.borrow().registers, [0x5a, 0xff, 0, 0xff]);
}

#[test]
fn pin_operations_preserve_other_bits_and_use_latch_not_input_feedback() {
    for index in 0..8 {
        let pin = Pin::new(index).unwrap();
        let (mut dev, state) = device([pin.mask(), !pin.mask(), 0x5a, 0xa5]);
        assert_eq!(run(dev.read_pin(pin)), Ok(Level::High));
        state.borrow_mut().registers[0] = !pin.mask();
        assert_eq!(run(dev.read_pin(pin)), Ok(Level::Low));
        // Physical feedback can differ from commanded levels (e.g. external load).
        state.borrow_mut().registers[0] = 0;
        run(dev.set_level(pin, Level::High)).unwrap();
        assert_eq!(state.borrow().registers[1], 0xff);
        run(dev.set_level(pin, Level::Low)).unwrap();
        assert_eq!(state.borrow().registers[1], !pin.mask());
        run(dev.set_direction(pin, Direction::Input)).unwrap();
        assert_eq!(state.borrow().registers[3], 0xa5 | pin.mask());
        run(dev.set_direction(pin, Direction::Output)).unwrap();
        assert_eq!(state.borrow().registers[3], 0xa5 & !pin.mask());
        run(dev.set_polarity(pin, Polarity::Inverted)).unwrap();
        assert_eq!(state.borrow().registers[2], 0x5a | pin.mask());
        run(dev.set_polarity(pin, Polarity::Normal)).unwrap();
        assert_eq!(state.borrow().registers[2], 0x5a & !pin.mask());
        assert_eq!(state.borrow().registers[1], !pin.mask());
    }
}

async fn update(dev: &mut Xca9534<Bus>, reg: usize, mask: u8, value: u8) -> Result<(), ErrorKind> {
    match reg {
        1 => dev.update_outputs(mask, value).await,
        2 => dev.update_polarity(mask, value).await,
        3 => dev.update_configuration(mask, value).await,
        _ => unreachable!(),
    }
}

#[test]
fn masked_updates_preserve_unselected_bits_and_ignore_unselected_values() {
    for reg in 1..=3 {
        for previous in [0, 0x55, 0xaa, 0xff] {
            for mask in 0..=u8::MAX {
                for value in [0, 0x69, 0xff] {
                    let (mut dev, state) = device([previous; 4]);
                    run(update(&mut dev, reg, mask, value)).unwrap();
                    let actual = state.borrow().registers[reg];
                    assert_eq!(actual & mask, value & mask);
                    assert_eq!(actual & !mask, previous & !mask);
                    if mask == 0 {
                        assert!(state.borrow().calls.is_empty());
                    } else {
                        assert_eq!(
                            state.borrow().calls,
                            [
                                Call::Read(0x20, reg as u8),
                                Call::Write(0x20, reg as u8, actual)
                            ]
                        );
                    }
                    for other in 0..4 {
                        if other != reg {
                            assert_eq!(state.borrow().registers[other], previous);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn reset_and_external_changes_are_observed_without_cache_invalidation() {
    let (mut dev, state) = device([0; 4]);
    run(dev.init(Config {
        outputs: 0x80,
        inputs: 0x55,
        polarity: 0xaa,
    }))
    .unwrap();
    // Simulated power cycle between completed calls, followed by explicit updates.
    state.borrow_mut().registers = [0, 0xff, 0, 0xff];
    run(dev.set_level(Pin::P0, Level::Low)).unwrap();
    run(dev.set_direction(Pin::P1, Direction::Output)).unwrap();
    run(dev.set_polarity(Pin::P2, Polarity::Inverted)).unwrap();
    assert_eq!(state.borrow().registers, [0, 0xfe, 4, 0xfd]);
}

#[test]
fn errors_are_preserved_and_failed_reads_never_produce_samples() {
    for error in [
        ErrorKind::Bus,
        ErrorKind::ArbitrationLoss,
        ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address),
        ErrorKind::NoAcknowledge(NoAcknowledgeSource::Data),
    ] {
        for after in [false, true] {
            for register in 0..4 {
                let (mut dev, state) = device([0xff; 4]);
                state.borrow_mut().failure = Some((0, after, error));
                let result = match register {
                    0 => run(dev.read_inputs()),
                    1 => run(dev.read_outputs()),
                    2 => run(dev.read_polarity()),
                    3 => run(dev.read_configuration()),
                    _ => unreachable!(),
                };
                assert_eq!(result, Err(error));
                assert_eq!(state.borrow().calls.len(), 1);
            }
            let (mut dev, state) = device([0xff; 4]);
            state.borrow_mut().failure = Some((0, after, error));
            assert_eq!(run(dev.read_pin(Pin::P0)), Err(error));
        }
    }
}

#[test]
fn update_errors_stop_without_retry_and_next_update_reads_actual_hardware() {
    for reg in 1..=3 {
        for transaction in 0..2 {
            for after in [false, true] {
                let (mut dev, state) = device([0; 4]);
                state.borrow_mut().failure = Some((transaction, after, ErrorKind::Bus));
                assert_eq!(run(update(&mut dev, reg, 1, 1)), Err(ErrorKind::Bus));
                assert_eq!(state.borrow().calls.len(), transaction + 1);
                let expected = u8::from(transaction == 1 && after);
                assert_eq!(state.borrow().registers[reg], expected);
                state.borrow_mut().failure = None;
                run(update(&mut dev, reg, 2, 2)).unwrap();
                assert_eq!(state.borrow().registers[reg], expected | 2);
                assert!(
                    matches!(state.borrow().calls[transaction + 1], Call::Read(_, r) if r == reg as u8)
                );
            }
        }
    }
}

#[test]
fn initialization_errors_stop_at_each_stage_and_can_be_explicitly_reapplied() {
    let config = Config {
        outputs: 0x12,
        polarity: 0x34,
        inputs: 0x56,
    };
    for transaction in 0..4 {
        for after in [false, true] {
            let (mut dev, state) = device([0; 4]);
            state.borrow_mut().failure = Some((transaction, after, ErrorKind::Bus));
            assert_eq!(run(dev.init(config)), Err(ErrorKind::Bus));
            assert_eq!(state.borrow().calls.len(), transaction + 1);
            state.borrow_mut().failure = None;
            run(dev.init(config)).unwrap();
            assert_eq!(state.borrow().registers, [0, 0x12, 0x34, 0x56]);
            assert_eq!(
                state.borrow().calls[transaction + 1],
                Call::Write(0x20, 3, 0xff)
            );
        }
    }
}

#[test]
fn cancellation_at_each_update_await_does_not_replay_or_use_stale_state() {
    for reg in 1..=3 {
        for polls in 1..=4 {
            let (mut dev, state) = device([0; 4]);
            cancel_after(update(&mut dev, reg, 1, 1), polls);
            let expected = u8::from(polls == 4);
            assert_eq!(state.borrow().registers[reg], expected);
            let calls = state.borrow().calls.len();
            // Caller has recovered the physical bus; next update must read anew.
            run(update(&mut dev, reg, 2, 2)).unwrap();
            assert_eq!(state.borrow().registers[reg], expected | 2);
            assert_eq!(state.borrow().calls[calls], Call::Read(0x20, reg as u8));
        }
    }
}

#[test]
fn cancellation_at_each_initialization_await_allows_explicit_reinitialization() {
    let config = Config {
        outputs: 0x12,
        polarity: 0x34,
        inputs: 0x56,
    };
    for polls in 1..=8 {
        let (mut dev, state) = device([0; 4]);
        cancel_after(dev.init(config), polls);
        let completed_effects = polls / 2;
        let mut expected = [0; 4];
        for &(reg, value) in [(3, 0xff), (1, 0x12), (2, 0x34), (3, 0x56)]
            .iter()
            .take(completed_effects)
        {
            expected[reg] = value;
        }
        assert_eq!(state.borrow().registers, expected);
        let calls = state.borrow().calls.len();
        run(dev.init(config)).unwrap();
        assert_eq!(state.borrow().registers, [0, 0x12, 0x34, 0x56]);
        assert_eq!(state.borrow().calls[calls], Call::Write(0x20, 3, 0xff));
    }
}

#[test]
fn cancelled_input_read_is_reselected_and_sampled_again() {
    for polls in 1..=2 {
        let (mut dev, state) = device([0xff; 4]);
        cancel_after(dev.read_inputs(), polls);
        state.borrow_mut().registers[0] = 0x42;
        assert_eq!(run(dev.read_inputs()), Ok(0x42));
        assert_eq!(
            state.borrow().calls,
            [Call::Read(0x20, 0), Call::Read(0x20, 0)]
        );
    }
}
