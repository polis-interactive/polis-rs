use super::*;
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use embedded_hal_async::i2c::{ErrorKind, ErrorType, NoAcknowledgeSource, Operation};
use std::collections::VecDeque;

#[derive(Debug)]
enum Step {
    Read(u8, Result<[u8; 4], ErrorKind>),
    Write(u8, [u8; 3], Result<(), ErrorKind>),
}

struct Bus(VecDeque<Step>);

impl Bus {
    fn new(steps: impl IntoIterator<Item = Step>) -> Self {
        Self(steps.into_iter().collect())
    }

    fn done(self) {
        assert!(self.0.is_empty(), "unconsumed bus operations: {:?}", self.0);
    }
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
        // Exercise suspension rather than relying on always-ready futures.
        let mut yielded = false;
        core::future::poll_fn(|cx| {
            if yielded {
                Poll::Ready(())
            } else {
                yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await;
        match self.0.pop_front().expect("unexpected bus operation") {
            Step::Read(expected_address, reply) => {
                assert_eq!(address, expected_address);
                let [Operation::Read(data)] = ops else {
                    panic!("expected a bare read, without a register-pointer write");
                };
                assert_eq!(data.len(), 4, "identifier plus all three buttons");
                // Model a bus that modifies the receive buffer before failing.
                data.fill(0xff);
                data.copy_from_slice(&reply?);
                Ok(())
            }
            Step::Write(expected_address, bytes, reply) => {
                assert_eq!(address, expected_address);
                let [Operation::Write(data)] = ops else {
                    panic!("expected exactly one LED write");
                };
                assert_eq!(*data, bytes);
                reply
            }
        }
    }
}

fn run<T>(future: impl Future<Output = T>) -> T {
    let mut future = core::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..10 {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
    panic!("future did not finish");
}

#[test]
fn reads_all_button_combinations_in_abc_order() {
    for mask in 0..8 {
        let levels = [mask & 1 != 0, mask & 2 != 0, mask & 4 != 0];
        let bus = Bus::new([Step::Read(
            0x3e,
            Ok([0x7c, levels[0] as u8, levels[1] as u8, levels[2] as u8]),
        )]);
        let mut buttons = ModulinoButtons::new(bus);
        let state = run(buttons.read_buttons()).unwrap();
        assert_eq!([state.a, state.b, state.c], levels);
        for (button, expected) in [Button::A, Button::B, Button::C].into_iter().zip(levels) {
            assert_eq!(state.is_pressed(button), expected);
        }
        buttons.into_inner().done();
    }
}

#[test]
fn writes_all_led_combinations_without_a_register_prefix() {
    for mask in 0..8 {
        let bytes = [mask & 1, (mask >> 1) & 1, (mask >> 2) & 1];
        let mut buttons = ModulinoButtons::new(Bus::new([Step::Write(0x3e, bytes, Ok(()))]));
        run(buttons.set_leds(bytes[0] != 0, bytes[1] != 0, bytes[2] != 0)).unwrap();
        buttons.into_inner().done();
    }
}

#[test]
fn supports_custom_addresses_with_fixed_pinstrap_id_and_borrowed_bus() {
    for address in [0x08, 0x3e, 0x42, 0x77] {
        let mut bus = Bus::new([
            Step::Read(address, Ok([0x7c, 1, 0, 1])),
            Step::Write(address, [0, 1, 0], Ok(())),
        ]);
        let mut buttons = ModulinoButtons::with_address(&mut bus, address).unwrap();
        assert_eq!(buttons.address(), address);
        assert!(run(buttons.read_buttons()).unwrap().a);
        run(buttons.set_leds(false, true, false)).unwrap();
        let _ = buttons.into_inner();
        bus.done();
    }
}

#[test]
fn constructors_and_bus_access_do_not_communicate() {
    let mut buttons = ModulinoButtons::new(Bus::new([]));
    assert_eq!(buttons.address(), 0x3e);
    assert!(buttons.bus_mut().0.is_empty());
    buttons.into_inner().done();
    ModulinoButtons::with_address(Bus::new([]), 0x42)
        .unwrap()
        .into_inner()
        .done();
}

#[test]
fn rejects_every_reserved_or_non_seven_bit_address_without_io() {
    for address in 0..=u8::MAX {
        let mut bus = Bus::new([]);
        let result = ModulinoButtons::with_address(&mut bus, address);
        if (0x08..=0x77).contains(&address) {
            assert!(result.is_ok());
        } else {
            assert!(matches!(result, Err(InvalidAddress(value)) if value == address));
        }
        bus.done();
    }
}

#[test]
fn nonzero_button_bytes_match_arduino_boolean_semantics() {
    let mut buttons = ModulinoButtons::new(Bus::new([Step::Read(0x3e, Ok([0x7c, 2, 128, 255]))]));
    assert_eq!(
        run(buttons.read_buttons()).unwrap(),
        ButtonState {
            a: true,
            b: true,
            c: true
        }
    );
    buttons.into_inner().done();
}

#[test]
fn rejects_wrong_module_then_accepts_a_valid_read() {
    let mut buttons = ModulinoButtons::new(Bus::new([
        Step::Read(0x3e, Ok([0x58, 1, 1, 1])),
        Step::Read(0x3e, Ok([0x7c, 0, 0, 0])),
    ]));
    assert_eq!(
        run(buttons.read_buttons()),
        Err(Error::UnexpectedDevice { pinstrap_id: 0x58 })
    );
    assert_eq!(run(buttons.read_buttons()), Ok(ButtonState::default()));
    buttons.into_inner().done();
}

#[test]
fn failed_reads_return_errors_even_after_a_successful_sample() {
    let nack = ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address);
    let mut buttons = ModulinoButtons::new(Bus::new([
        Step::Read(0x3e, Ok([0x7c, 1, 0, 0])),
        Step::Read(0x3e, Err(nack)),
        Step::Read(0x3e, Err(ErrorKind::Bus)),
        Step::Read(0x3e, Ok([0x7c, 0, 1, 0])),
    ]));
    assert!(run(buttons.read_buttons()).unwrap().a);
    assert_eq!(run(buttons.read_buttons()), Err(Error::I2c(nack)));
    assert_eq!(run(buttons.read_buttons()), Err(Error::I2c(ErrorKind::Bus)));
    assert_eq!(
        run(buttons.read_buttons()),
        Ok(ButtonState {
            a: false,
            b: true,
            c: false
        })
    );
    buttons.into_inner().done();
}

#[test]
fn led_errors_propagate_without_retry_or_skipping_explicit_reapplication() {
    let nack = ErrorKind::NoAcknowledge(NoAcknowledgeSource::Data);
    let mut buttons = ModulinoButtons::new(Bus::new([
        Step::Write(0x3e, [1, 0, 1], Err(nack)),
        Step::Write(0x3e, [1, 0, 1], Ok(())),
        Step::Write(0x3e, [1, 0, 1], Ok(())),
    ]));
    assert_eq!(
        run(buttons.set_leds(true, false, true)),
        Err(Error::I2c(nack))
    );
    run(buttons.set_leds(true, false, true)).unwrap();
    run(buttons.set_leds(true, false, true)).unwrap();
    buttons.into_inner().done();
}
