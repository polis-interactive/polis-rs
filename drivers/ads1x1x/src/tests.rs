use super::*;
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use embedded_hal::i2c::{ErrorKind, ErrorType, I2c, Operation};
use std::{collections::VecDeque, vec::Vec};

#[derive(Debug)]
enum Step {
    Read(u8, Result<u16, ErrorKind>),
    Write([u8; 3], Result<(), ErrorKind>),
}
struct Bus {
    address: u8,
    steps: VecDeque<Step>,
}
impl Bus {
    fn new(steps: impl IntoIterator<Item = Step>) -> Self {
        Self {
            address: 0x48,
            steps: steps.into_iter().collect(),
        }
    }
    fn done(self) {
        assert!(self.steps.is_empty(), "unused transfers: {:?}", self.steps);
    }
}
impl ErrorType for Bus {
    type Error = ErrorKind;
}
impl I2c for Bus {
    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), ErrorKind> {
        assert_eq!(address, self.address);
        match self.steps.pop_front().expect("unexpected bus I/O") {
            Step::Read(register, result) => {
                let [Operation::Write(pointer), Operation::Read(output)] = operations else {
                    panic!("expected repeated START register read");
                };
                assert_eq!(*pointer, &[register]);
                output.copy_from_slice(&result?.to_be_bytes());
                Ok(())
            }
            Step::Write(expected, result) => {
                let [Operation::Write(bytes)] = operations else {
                    panic!("expected register write");
                };
                assert_eq!(*bytes, expected);
                result
            }
        }
    }
}
impl embedded_hal_async::i2c::I2c for Bus {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), ErrorKind> {
        // Suspend every transaction so cancellation tests cross real await points.
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
        I2c::transaction(self, address, operations)
    }
}
#[derive(Default)]
struct Delay(Vec<u32>);
impl embedded_hal::delay::DelayNs for Delay {
    fn delay_ns(&mut self, ns: u32) {
        self.0.push(ns);
    }
}
impl embedded_hal_async::delay::DelayNs for Delay {
    async fn delay_ns(&mut self, ns: u32) {
        self.0.push(ns);
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
fn read(register: u8, value: u16) -> Step {
    Step::Read(register, Ok(value))
}
fn write(bytes: [u8; 3]) -> Step {
    Step::Write(bytes, Ok(()))
}

fn exercise_model<M: Model>() {
    for asynchronous in [false, true] {
        let mut adc = Ads1x1x::<_, M>::new(
            Bus::new([
                // No OS bit in a plain configure, even though reset reads 0x8583.
                write([1, 0x05, 0x83]),
                read(0, 0x8000),
                read(0, 0x7fff),
                read(0, 0xfff0),
                read(1, 0x8583),
                write([1, 0x85, 0x83]),
                read(1, 0x0583),
                read(1, 0x8583),
                read(0, 0x0010),
            ]),
            Address::Ground,
        );
        let mut delay = Delay::default();
        if asynchronous {
            run(adc.configure_async(Config::default())).unwrap();
        } else {
            adc.configure(Config::default()).unwrap();
        }
        for raw in [i16::MIN, i16::MAX, -16] {
            let sample = if asynchronous {
                run(adc.read_conversion_async())
            } else {
                adc.read_conversion()
            };
            assert_eq!(sample.unwrap(), raw >> (16 - M::BITS));
        }
        let sample = if asynchronous {
            run(adc.read_one_shot_async(Channel::Differential01, &mut delay, 2))
        } else {
            adc.read_one_shot(Channel::Differential01, &mut delay, 2)
        };
        assert_eq!(sample.unwrap(), 16 >> (16 - M::BITS));
        assert_eq!(delay.0, [1_000_000, 1_000_000]);
        adc.into_inner().done();
    }
}
#[test]
fn six_models_blocking_and_async_wire_fixtures() {
    exercise_model::<model::Ads1013>();
    exercise_model::<model::Ads1014>();
    exercise_model::<model::Ads1015>();
    exercise_model::<model::Ads1113>();
    exercise_model::<model::Ads1114>();
    exercise_model::<model::Ads1115>();
}

fn rejected<M: Model>(config: Config<M>, expected: Error<ErrorKind>) {
    let mut adc = Ads1x1x::<_, M>::new(Bus::new([]), Address::Ground);
    assert_eq!(adc.configure(config), Err(expected));
    assert_eq!(run(adc.configure_async(config)), Err(expected));
    adc.into_inner().done();
}
fn fixed_input<M: Model>() {
    rejected::<M>(
        Config {
            channel: Channel::Single0,
            ..Default::default()
        },
        Error::UnsupportedChannel,
    );
    let mut adc = Ads1x1x::<_, M>::new(Bus::new([]), Address::Ground);
    assert_eq!(
        adc.start_one_shot(Channel::Single1),
        Err(Error::UnsupportedChannel)
    );
    assert_eq!(
        run(adc.start_one_shot_async(Channel::Single1)),
        Err(Error::UnsupportedChannel)
    );
    adc.into_inner().done();
}
#[test]
fn unsupported_features_rejected_without_io() {
    fixed_input::<model::Ads1013>();
    fixed_input::<model::Ads1014>();
    fixed_input::<model::Ads1113>();
    fixed_input::<model::Ads1114>();
    rejected::<model::Ads1013>(
        Config {
            range: FullScaleRange::V4_096,
            ..Default::default()
        },
        Error::UnsupportedRange,
    );
    rejected::<model::Ads1113>(
        Config {
            range: FullScaleRange::V0_256,
            ..Default::default()
        },
        Error::UnsupportedRange,
    );
    rejected::<model::Ads1013>(
        Config {
            alert: Alert::Ready { active_high: false },
            ..Default::default()
        },
        Error::UnsupportedAlert,
    );
    rejected::<model::Ads1113>(
        Config {
            alert: Alert::Comparator {
                config: Comparator::default(),
                low: 0,
                high: 1,
            },
            ..Default::default()
        },
        Error::UnsupportedAlert,
    );
}

#[test]
fn comparator_thresholds_are_scaled_and_disabled_during_configuration() {
    for asynchronous in [false, true] {
        let mut adc = Ads1015::new(
            Bus::new([
                write([1, 0x66, 0xdf]),
                write([2, 0x80, 0x00]),
                write([3, 0x7f, 0xf0]),
                write([1, 0x66, 0xde]),
            ]),
            Address::Ground,
        );
        let config = Config {
            channel: Channel::Single2,
            range: FullScaleRange::V1_024,
            mode: Mode::Continuous,
            data_rate: model::DataRate12Bit::Sps3300,
            alert: Alert::Comparator {
                config: Comparator {
                    window: true,
                    active_high: true,
                    latching: true,
                    queue: Queue::Four,
                },
                low: -2048,
                high: 2047,
            },
        };
        if asynchronous {
            run(adc.configure_async(config)).unwrap();
        } else {
            adc.configure(config).unwrap();
        }
        adc.into_inner().done();
    }
    rejected::<model::Ads1014>(
        Config {
            alert: Alert::Comparator {
                config: Comparator::default(),
                low: -2049,
                high: 0,
            },
            ..Default::default()
        },
        Error::InvalidThresholds,
    );
    rejected::<model::Ads1015>(
        Config {
            alert: Alert::Comparator {
                config: Comparator::default(),
                low: 0,
                high: 2048,
            },
            ..Default::default()
        },
        Error::InvalidThresholds,
    );
    rejected::<model::Ads1115>(
        Config {
            alert: Alert::Comparator {
                config: Comparator::default(),
                low: 1,
                high: 0,
            },
            ..Default::default()
        },
        Error::InvalidThresholds,
    );
}

fn alert_modes<M: Model>() {
    for asynchronous in [false, true] {
        let mut adc = Ads1x1x::<_, M>::new(
            Bus::new([
                // Ready thresholds are raw register patterns, including on 12-bit ADCs.
                write([1, 0x05, 0x8b]),
                write([2, 0, 0]),
                write([3, 0x80, 0]),
                write([1, 0x05, 0x88]),
                write([1, 0x05, 0x83]),
                write([2, 0, 0]),
                write([3, 0x80, 0]),
                write([1, 0x05, 0x80]),
                write([1, 0x05, 0x83]),
            ]),
            Address::Ground,
        );
        for alert in [
            Alert::Ready { active_high: true },
            Alert::Ready { active_high: false },
            Alert::Disabled,
        ] {
            let config = Config {
                alert,
                ..Default::default()
            };
            if asynchronous {
                run(adc.configure_async(config)).unwrap();
            } else {
                adc.configure(config).unwrap();
            }
        }
        adc.into_inner().done();
    }
}
#[test]
fn ready_modes_on_all_four_capable_models() {
    alert_modes::<model::Ads1014>();
    alert_modes::<model::Ads1015>();
    alert_modes::<model::Ads1114>();
    alert_modes::<model::Ads1115>();
}

#[test]
fn full_16_bit_thresholds_and_rates() {
    for asynchronous in [false, true] {
        let mut adc = Ads1114::new(
            Bus::new([
                write([1, 0x05, 0xe3]),
                write([2, 0x80, 0]),
                write([3, 0x7f, 0xff]),
                write([1, 0x05, 0xe0]),
            ]),
            Address::Ground,
        );
        let config = Config {
            data_rate: model::DataRate16Bit::Sps860,
            alert: Alert::Comparator {
                config: Comparator::default(),
                low: i16::MIN,
                high: i16::MAX,
            },
            ..Default::default()
        };
        if asynchronous {
            run(adc.configure_async(config)).unwrap();
        } else {
            adc.configure(config).unwrap();
        }
        adc.into_inner().done();
    }
}

#[test]
fn rates_have_independent_family_encodings() {
    use model::{DataRate12Bit as R12, DataRate16Bit as R16};
    for (rate, expected) in [
        (R12::Sps128, 0x0503),
        (R12::Sps250, 0x0523),
        (R12::Sps490, 0x0543),
        (R12::Sps920, 0x0563),
        (R12::Sps1600, 0x0583),
        (R12::Sps2400, 0x05a3),
        (R12::Sps3300, 0x05c3),
    ] {
        assert_eq!(
            Config::<model::Ads1015> {
                data_rate: rate,
                ..Default::default()
            }
            .registers::<()>()
            .unwrap()
            .bits,
            expected
        );
    }
    for (rate, expected) in [
        (R16::Sps8, 0x0503),
        (R16::Sps16, 0x0523),
        (R16::Sps32, 0x0543),
        (R16::Sps64, 0x0563),
        (R16::Sps128, 0x0583),
        (R16::Sps250, 0x05a3),
        (R16::Sps475, 0x05c3),
        (R16::Sps860, 0x05e3),
    ] {
        assert_eq!(
            Config::<model::Ads1115> {
                data_rate: rate,
                ..Default::default()
            }
            .registers::<()>()
            .unwrap()
            .bits,
            expected
        );
    }
}

#[test]
fn triggers_preserve_settings_and_select_each_mux_channel() {
    for (channel, high) in [
        (Channel::Differential01, 0x89),
        (Channel::Differential03, 0x99),
        (Channel::Differential13, 0xa9),
        (Channel::Differential23, 0xb9),
        (Channel::Single0, 0xc9),
        (Channel::Single1, 0xd9),
        (Channel::Single2, 0xe9),
        (Channel::Single3, 0xf9),
    ] {
        for asynchronous in [false, true] {
            let mut adc = Ads1115::new(
                Bus::new([read(1, 0xe92c), write([1, high, 0x2c])]),
                Address::Ground,
            );
            if asynchronous {
                run(adc.start_one_shot_async(channel)).unwrap();
            } else {
                adc.start_one_shot(channel).unwrap();
            }
            adc.into_inner().done();
        }
    }
}

#[test]
fn polling_is_bounded_and_checks_hardware_mode() {
    for asynchronous in [false, true] {
        let mut adc = Ads1015::new(
            Bus::new([
                read(1, 0x0483), // continuous
                read(1, 0x0583), // busy single-shot
                read(1, 0x8583),
                write([1, 0xc5, 0x83]),
                read(1, 0x4583),
                read(1, 0x4583), // timeout
                read(1, 0x0483), // mode changed outside driver
            ]),
            Address::Ground,
        );
        let mut delay = Delay::default();
        let zero = if asynchronous {
            run(adc.read_one_shot_async(Channel::Single0, &mut delay, 0))
        } else {
            adc.read_one_shot(Channel::Single0, &mut delay, 0)
        };
        assert_eq!(zero, Err(Error::InvalidPollLimit));
        for expected in [Error::NotSingleShot, Error::Busy, Error::Timeout] {
            let result = if asynchronous {
                run(adc.read_one_shot_async(Channel::Single0, &mut delay, 2))
            } else {
                adc.read_one_shot(Channel::Single0, &mut delay, 2)
            };
            assert_eq!(result, Err(expected));
        }
        assert_eq!(delay.0, [1_000_000, 1_000_000]);
        let result = if asynchronous {
            run(adc.one_shot_ready_async())
        } else {
            adc.one_shot_ready()
        };
        assert_eq!(result, Err(Error::NotSingleShot));
        adc.into_inner().done();
    }
}

#[test]
fn all_addresses_and_transport_errors() {
    for address in [Address::Ground, Address::Supply, Address::Sda, Address::Scl] {
        let mut bus = Bus::new([
            Step::Read(0, Err(ErrorKind::Bus)),
            Step::Read(0, Err(ErrorKind::ArbitrationLoss)),
            Step::Write([1, 5, 0x83], Err(ErrorKind::Bus)),
            Step::Write([1, 5, 0x83], Err(ErrorKind::ArbitrationLoss)),
        ]);
        bus.address = address as u8;
        let mut adc = Ads1013::new(bus, address);
        assert_eq!(adc.read_conversion(), Err(Error::I2c(ErrorKind::Bus)));
        assert_eq!(
            run(adc.read_conversion_async()),
            Err(Error::I2c(ErrorKind::ArbitrationLoss))
        );
        assert_eq!(
            adc.configure(Config::default()),
            Err(Error::I2c(ErrorKind::Bus))
        );
        assert_eq!(
            run(adc.configure_async(Config::default())),
            Err(Error::I2c(ErrorKind::ArbitrationLoss))
        );
        adc.into_inner().done();
    }
}

#[test]
fn cancelled_configuration_can_be_reapplied_without_cached_state() {
    let mut adc = Ads1115::new(
        Bus::new([
            write([1, 5, 0x83]), // cancelled after comparator disable
            write([1, 5, 0x83]),
            write([2, 0, 0]),
            write([3, 0x80, 0]),
            write([1, 5, 0x80]),
        ]),
        Address::Ground,
    );
    let config = Config {
        alert: Alert::Ready { active_high: false },
        ..Default::default()
    };
    {
        let mut future = core::pin::pin!(adc.configure_async(config));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut cx).is_pending());
        assert!(future.as_mut().poll(&mut cx).is_pending());
    }
    run(adc.configure_async(config)).unwrap();
    adc.into_inner().done();
}

#[test]
fn failed_threshold_write_stops_before_enabling_and_can_be_retried() {
    for asynchronous in [false, true] {
        let mut adc = Ads1014::new(
            Bus::new([
                write([1, 5, 0x83]),
                Step::Write([2, 0, 0], Err(ErrorKind::Bus)),
                write([1, 5, 0x83]),
                write([2, 0, 0]),
                write([3, 0x80, 0]),
                write([1, 5, 0x80]),
            ]),
            Address::Ground,
        );
        let config = Config {
            alert: Alert::Ready { active_high: false },
            ..Default::default()
        };
        let result = if asynchronous {
            run(adc.configure_async(config))
        } else {
            adc.configure(config)
        };
        assert_eq!(result, Err(Error::I2c(ErrorKind::Bus)));
        if asynchronous {
            run(adc.configure_async(config)).unwrap();
        } else {
            adc.configure(config).unwrap();
        }
        adc.into_inner().done();
    }
}

#[test]
fn signed_scaling_does_not_overflow_at_full_range() {
    assert_eq!(
        Ads1015::<()>::microvolts(-2048, FullScaleRange::V6_144),
        -6_144_000
    );
    assert_eq!(
        Ads1015::<()>::microvolts(2047, FullScaleRange::V2_048),
        2_047_000
    );
    assert_eq!(
        Ads1115::<()>::microvolts(i16::MIN, FullScaleRange::V6_144),
        -6_144_000
    );
    assert_eq!(
        Ads1115::<()>::microvolts(i16::MAX, FullScaleRange::V6_144),
        6_143_812
    );
    assert_eq!(Ads1113::<()>::microvolts(-1, FullScaleRange::V2_048), -62);
}
