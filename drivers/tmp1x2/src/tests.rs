use super::*;
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use embedded_hal_async::i2c::{ErrorKind, ErrorType, NoAcknowledgeSource, Operation};
use std::{collections::VecDeque, vec::Vec};

#[derive(Debug)]
enum Step {
    Read(u8, u8, Result<[u8; 2], ErrorKind>),
    Write(u8, Vec<u8>, Result<(), ErrorKind>),
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
        // Every transfer suspends, so tests exercise real async composition.
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
            Step::Read(expected_address, register, reply) => {
                assert_eq!(address, expected_address);
                let [Operation::Write(pointer), Operation::Read(data)] = ops else {
                    panic!("expected a single repeated-START write/read transaction");
                };
                assert_eq!(*pointer, &[register]);
                assert_eq!(data.len(), 2);
                data.copy_from_slice(&reply?);
                Ok(())
            }
            Step::Write(expected_address, bytes, reply) => {
                assert_eq!(address, expected_address);
                let [Operation::Write(data)] = ops else {
                    panic!("expected one register write");
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
    for _ in 0..100 {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
    panic!("future did not finish");
}

fn read(register: u8, bytes: [u8; 2]) -> Step {
    Step::Read(0x48, register, Ok(bytes))
}

fn write(bytes: [u8; 3]) -> Step {
    Step::Write(0x48, bytes.to_vec(), Ok(()))
}

#[test]
fn normal_and_extended_samples_preserve_sign_and_resolution() {
    // Shared fixtures from TMP102 SBOS397I section 6.5.2 and TMP112 SBOS473L
    // section 7.5.2, including negative fractions and >128 C. Both devices use
    // these exact wire formats; model-specific code paths are unnecessary.
    for (bytes, sixteenths) in [
        ([0x00, 0x00], 0),
        ([0x00, 0x10], 1),
        ([0xff, 0xf0], -1),
        ([0x19, 0x00], 400),
        ([0xe7, 0x00], -400),
        ([0xc9, 0x00], -880),
        ([0x7d, 0x00], 2000),
        ([0x00, 0x09], 1),
        ([0xff, 0xf9], -1),
        ([0x0c, 0x81], 400),
        ([0xf3, 0x81], -400),
        ([0x4b, 0x01], 2400),
    ] {
        let mut sensor = Tmp1x2::new(Bus::new([read(0, bytes)]), Address::Ground);
        let temperature = run(sensor.read_temperature()).unwrap();
        assert_eq!(temperature.sixteenths_celsius(), sixteenths);
        sensor.into_inner().done();
    }
    let negative_fraction = Temperature::from_register(0xfff0);
    assert_eq!(negative_fraction.micro_celsius(), -62_500);
    assert_eq!(negative_fraction.milli_celsius(), -62);
    assert_eq!(Temperature::from_register(0x1900).milli_celsius(), 25_000);
}

#[test]
fn supports_all_addresses_and_borrowed_buses_without_constructor_io() {
    for (address, wire) in [
        (Address::Ground, 0x48),
        (Address::Supply, 0x49),
        (Address::Sda, 0x4a),
        (Address::Scl, 0x4b),
    ] {
        let mut bus = Bus::new([Step::Read(wire, 0, Ok([0x19, 0]))]);
        run(Tmp1x2::new(&mut bus, address).read_temperature()).unwrap();
        bus.done();
    }
    Tmp1x2::new(Bus::new([]), Address::Ground)
        .into_inner()
        .done();
}

#[test]
fn reads_default_config_and_all_conversion_rates() {
    let mut sensor = Tmp1x2::new(Bus::new([read(1, [0x60, 0xa0])]), Address::Ground);
    assert_eq!(run(sensor.read_config()).unwrap(), Config::default());
    sensor.into_inner().done();
    for (lsb, rate) in [
        (0x30, ConversionRate::QuarterHz),
        (0x70, ConversionRate::Hz1),
        (0xb0, ConversionRate::Hz4),
        (0xf0, ConversionRate::Hz8),
    ] {
        let mut sensor = Tmp1x2::new(Bus::new([read(1, [0xe1, lsb])]), Address::Ground);
        assert_eq!(
            run(sensor.read_config()).unwrap(),
            Config {
                conversion_rate: rate,
                shutdown: true,
                extended_mode: true,
            }
        );
        sensor.into_inner().done();
    }
}

#[test]
fn configuration_preserves_alert_controls_without_retriggering_one_shot() {
    let mut sensor = Tmp1x2::new(
        Bus::new([
            read(1, [0xff, 0xff]),
            write([1, 0x1f, 0x50]),
            read(1, [0xff, 0x70]),
            write([1, 0x1e, 0x80]),
        ]),
        Address::Ground,
    );
    run(sensor.configure(Config {
        conversion_rate: ConversionRate::Hz1,
        shutdown: true,
        extended_mode: true,
    }))
    .unwrap();
    run(sensor.configure(Config::default())).unwrap();
    sensor.into_inner().done();
}

#[test]
fn writes_each_conversion_rate() {
    for (lsb, rate) in [
        (0x00, ConversionRate::QuarterHz),
        (0x40, ConversionRate::Hz1),
        (0x80, ConversionRate::Hz4),
        (0xc0, ConversionRate::Hz8),
    ] {
        let mut sensor = Tmp1x2::new(
            Bus::new([read(1, [0x60, 0xa0]), write([1, 0, lsb])]),
            Address::Ground,
        );
        run(sensor.configure(Config {
            conversion_rate: rate,
            ..Config::default()
        }))
        .unwrap();
        sensor.into_inner().done();
    }
}

#[test]
fn one_shot_trigger_busy_ready_and_sample() {
    let mut sensor = Tmp1x2::new(
        Bus::new([
            read(1, [0xe1, 0xb0]),
            write([1, 0x81, 0x90]),
            read(1, [0x61, 0xb0]),
            read(1, [0xe1, 0xb0]),
            read(0, [0xf3, 0x81]),
        ]),
        Address::Ground,
    );
    run(sensor.start_one_shot()).unwrap();
    assert!(!run(sensor.one_shot_ready()).unwrap());
    assert!(run(sensor.one_shot_ready()).unwrap());
    assert_eq!(
        run(sensor.read_temperature()).unwrap().milli_celsius(),
        -25_000
    );
    sensor.into_inner().done();
}

#[test]
fn one_shot_rejects_continuous_mode_and_busy_conversion_without_writes() {
    let mut sensor = Tmp1x2::new(
        Bus::new([
            read(1, [0xe0, 0xa0]),
            read(1, [0xe0, 0xa0]),
            read(1, [0x61, 0xa0]),
        ]),
        Address::Ground,
    );
    assert_eq!(run(sensor.start_one_shot()), Err(Error::NotShutdown));
    assert_eq!(run(sensor.one_shot_ready()), Err(Error::NotShutdown));
    assert_eq!(run(sensor.start_one_shot()), Err(Error::Busy));
    sensor.into_inner().done();
}

#[test]
fn transport_errors_propagate_and_failed_reads_do_not_write() {
    let nack = ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address);
    let mut sensor = Tmp1x2::new(
        Bus::new([
            Step::Read(0x48, 0, Err(nack)),
            Step::Read(0x48, 1, Err(nack)),
            Step::Read(0x48, 1, Err(nack)),
            Step::Read(0x48, 1, Err(nack)),
            Step::Read(0x48, 1, Err(nack)),
        ]),
        Address::Ground,
    );
    assert_eq!(run(sensor.read_temperature()), Err(Error::I2c(nack)));
    assert_eq!(run(sensor.read_config()), Err(Error::I2c(nack)));
    assert_eq!(
        run(sensor.configure(Config::default())),
        Err(Error::I2c(nack))
    );
    assert_eq!(run(sensor.start_one_shot()), Err(Error::I2c(nack)));
    assert_eq!(run(sensor.one_shot_ready()), Err(Error::I2c(nack)));
    sensor.into_inner().done();
}

#[test]
fn failed_config_write_does_not_cache_the_requested_mode() {
    let mut sensor = Tmp1x2::new(
        Bus::new([
            read(1, [0x60, 0xa0]),
            Step::Write(0x48, [1, 0x01, 0x90].to_vec(), Err(ErrorKind::Bus)),
            // Model a partial write: shutdown reached the device, extended mode did not.
            read(1, [0xe1, 0xa0]),
            read(0, [0xe7, 0x00]),
        ]),
        Address::Ground,
    );
    assert_eq!(
        run(sensor.configure(Config {
            shutdown: true,
            extended_mode: true,
            ..Config::default()
        })),
        Err(Error::I2c(ErrorKind::Bus))
    );
    assert_eq!(
        run(sensor.read_config()).unwrap(),
        Config {
            shutdown: true,
            ..Config::default()
        }
    );
    assert_eq!(
        run(sensor.read_temperature()).unwrap().milli_celsius(),
        -25_000
    );
    sensor.into_inner().done();
}

#[test]
fn cancelled_configuration_can_be_retried_from_device_state() {
    let mut sensor = Tmp1x2::new(
        Bus::new([
            read(1, [0x60, 0xa0]),
            // The mock suspends before beginning the write, which will be cancelled.
            read(1, [0x60, 0xa0]),
            write([1, 0, 0x80]),
        ]),
        Address::Ground,
    );
    {
        let mut future = core::pin::pin!(sensor.configure(Config {
            shutdown: true,
            ..Config::default()
        }));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut cx).is_pending());
        assert!(future.as_mut().poll(&mut cx).is_pending());
    }
    run(sensor.configure(Config::default())).unwrap();
    sensor.into_inner().done();
}
