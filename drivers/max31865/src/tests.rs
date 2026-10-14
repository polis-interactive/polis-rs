use super::*;
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use embedded_hal_async::spi::{ErrorKind, ErrorType, Operation};
use std::{cell::RefCell, collections::VecDeque, rc::Rc, vec, vec::Vec};

#[derive(Debug)]
enum Step {
    Read(u8, Result<Vec<u8>, ErrorKind>),
    Write(Vec<u8>, Result<(), ErrorKind>),
    Delay(u32),
}

#[derive(Clone)]
struct Trace(Rc<RefCell<VecDeque<Step>>>);

impl Trace {
    fn new(steps: impl IntoIterator<Item = Step>) -> Self {
        Self(Rc::new(RefCell::new(steps.into_iter().collect())))
    }
    fn next(&self) -> Step {
        self.0
            .borrow_mut()
            .pop_front()
            .expect("unexpected operation")
    }
    fn done(&self) {
        assert!(self.0.borrow().is_empty(), "unconsumed: {:?}", self.0);
    }
}

struct Bus(Trace);
struct Delay(Trace);

async fn suspend() {
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
}

impl ErrorType for Bus {
    type Error = ErrorKind;
}

impl SpiDevice for Bus {
    async fn transaction(
        &mut self,
        operations: &mut [Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        suspend().await;
        match self.0.next() {
            Step::Read(address, result) => {
                let [Operation::Write(command), Operation::Read(data)] = operations else {
                    panic!("address and read must share one CS transaction")
                };
                assert_eq!(*command, &[address]);
                data.copy_from_slice(&result?);
                Ok(())
            }
            Step::Write(expected, result) => {
                let [Operation::Write(data)] = operations else {
                    panic!("expected register write")
                };
                assert_eq!(*data, expected);
                result
            }
            step => panic!("expected SPI operation, got {step:?}"),
        }
    }
}

impl DelayNs for Delay {
    async fn delay_ns(&mut self, ns: u32) {
        suspend().await;
        let Step::Delay(expected) = self.0.next() else {
            panic!("expected delay")
        };
        assert_eq!(ns, expected * 1_000_000);
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

fn read(address: u8, data: &[u8]) -> Step {
    Step::Read(address, Ok(data.to_vec()))
}
fn write(data: &[u8]) -> Step {
    Step::Write(data.to_vec(), Ok(()))
}
fn sensor(trace: &Trace) -> Max31865<Bus> {
    Max31865::new(Bus(trace.clone()), Config::pt100_3wire()).unwrap()
}

fn one_shot(idle: u8, settling: u32) -> Vec<Step> {
    vec![
        read(0, &[idle]),
        write(&[0x80, idle | 0x82]),
        Step::Delay(settling),
        write(&[0x80, idle | 0xa0]),
        Step::Delay(66),
        read(1, &[0x40, 0]),
        write(&[0x80, idle]),
    ]
}

#[test]
fn construction_is_io_free_and_supports_borrowed_devices() {
    let trace = Trace::new([]);
    let mut bus = Bus(trace.clone());
    let device = Max31865::new(&mut bus, Config::pt1000_3wire()).unwrap();
    assert_eq!(device.config(), &Config::pt1000_3wire());
    let _ = device.into_inner();
    trace.done();
}

#[test]
fn initialization_encodes_all_wiring_and_filter_combinations() {
    for (wires, wire_bits) in [
        (WireCount::Two, 0),
        (WireCount::Three, 0x10),
        (WireCount::Four, 0),
    ] {
        for (filter, filter_bits) in [(FilterHz::Hz50, 1), (FilterHz::Hz60, 0)] {
            let trace = Trace::new([
                write(&[0x80, wire_bits | filter_bits | 2]),
                write(&[0x83, 0xff, 0xfe, 0, 0]),
            ]);
            let mut device = Max31865::new(
                Bus(trace.clone()),
                Config {
                    wires,
                    filter,
                    ..Config::pt100_3wire()
                },
            )
            .unwrap();
            run(device.init()).unwrap();
            trace.done();
        }
    }
}

#[test]
fn invalid_calibration_is_rejected_without_io() {
    for value in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for cfg in [
            Config {
                rtd_nominal: RtdNominal(value),
                ..Config::pt100_3wire()
            },
            Config {
                ref_resistor: RefResistor(value),
                ..Config::pt100_3wire()
            },
        ] {
            let trace = Trace::new([]);
            assert!(matches!(
                Max31865::new(Bus(trace.clone()), cfg),
                Err(Error::InvalidConfig)
            ));
            assert_eq!(
                run(sensor(&trace).configure(cfg)),
                Err(Error::InvalidConfig)
            );
            trace.done();
        }
    }
}

#[test]
fn failed_configuration_does_not_commit_calibration_and_can_be_reapplied() {
    let trace = Trace::new([
        Step::Write(vec![0x80, 0x13], Err(ErrorKind::Other)),
        write(&[0x80, 0x13]),
    ]);
    let mut device = sensor(&trace);
    let cfg = Config {
        filter: FilterHz::Hz50,
        ..Config::pt1000_3wire()
    };
    assert_eq!(
        run(device.configure(cfg)),
        Err(Error::Spi(ErrorKind::Other))
    );
    assert_eq!(device.config(), &Config::pt100_3wire());
    run(device.configure(cfg)).unwrap();
    assert_eq!(device.config(), &cfg);
    trace.done();
}

#[test]
fn persistent_controls_do_not_replay_command_bits() {
    let trace = Trace::new([
        read(0, &[0xff]),
        write(&[0x80, 0xd1]),
        read(0, &[0xff]),
        write(&[0x80, 0x51]),
        read(0, &[0xbf]),
        write(&[0x80, 0xd1]),
        read(0, &[0xff]),
        write(&[0x80, 0x91]),
        read(0, &[0xff]),
        write(&[0x80, 0xd3]),
    ]);
    let mut device = sensor(&trace);
    run(device.enable_bias(true)).unwrap();
    run(device.enable_bias(false)).unwrap();
    run(device.auto_convert(true)).unwrap();
    run(device.auto_convert(false)).unwrap();
    run(device.clear_fault()).unwrap();
    trace.done();
}

#[test]
fn thresholds_use_adc_counts_and_one_big_endian_transaction() {
    let trace = Trace::new([
        write(&[0x83, 0x46, 0x8a, 0x24, 0x68]),
        read(5, &[0x24, 0x69]),
        read(3, &[0x46, 0x8b]),
    ]);
    let mut device = sensor(&trace);
    for (low, high) in [(2, 1), (0, 0x8000), (0xffff, 0xffff)] {
        assert_eq!(
            run(device.set_thresholds(low, high)),
            Err(Error::InvalidThresholds)
        );
    }
    run(device.set_thresholds(0x1234, 0x2345)).unwrap();
    assert_eq!(run(device.lower_threshold()).unwrap(), 0x1234);
    assert_eq!(run(device.upper_threshold()).unwrap(), 0x2345);
    trace.done();
}

#[test]
fn sample_decodes_datasheet_ratios_and_preserves_all_fault_flags() {
    for (bytes, count) in [
        ([0x06, 0x66], 819),
        ([0x20, 0], 4096),
        ([0x40, 0], 8192),
        ([0x80, 0], 16384),
        ([0xc0, 0], 24576),
        ([0xff, 0xfe], 32767),
    ] {
        let trace = Trace::new([read(1, &bytes)]);
        assert_eq!(run(sensor(&trace).read_sample()).unwrap(), count);
        trace.done();
    }
    for status in [0, 4, 8, 0x10, 0x20, 0x40, 0x80, 0xff] {
        let trace = Trace::new([read(1, &[0x40, 1]), read(7, &[status])]);
        assert_eq!(
            run(sensor(&trace).read_sample()),
            Err(Error::Fault(FaultStatus::from_bits_truncate(status)))
        );
        trace.done();
    }
}

#[test]
fn one_shot_waits_for_both_filters_and_shuts_bias_off() {
    for idle in [0, 1, 0x10, 0x11] {
        let trace = Trace::new(one_shot(idle, 11));
        assert_eq!(
            run(sensor(&trace).read_rtd(&mut Delay(trace.clone()))).unwrap(),
            8192
        );
        trace.done();
    }
    for settling in [0, 25] {
        let trace = Trace::new(one_shot(0x10, settling.max(11)));
        run(sensor(&trace).read_rtd_with_settling(&mut Delay(trace.clone()), settling)).unwrap();
        trace.done();
    }
}

#[test]
fn one_shot_rejects_continuous_mode_without_writing() {
    let trace = Trace::new([read(0, &[0xd1])]);
    assert_eq!(
        run(sensor(&trace).read_rtd(&mut Delay(trace.clone()))),
        Err(Error::ContinuousMode)
    );
    trace.done();
}

#[test]
fn faulted_one_shot_reports_status_and_cleans_up() {
    let mut steps = one_shot(0x11, 11);
    steps[5] = read(1, &[0x40, 1]);
    steps.insert(6, read(7, &[0xa4]));
    let trace = Trace::new(steps);
    assert_eq!(
        run(sensor(&trace).read_temperature(&mut Delay(trace.clone()))),
        Err(Error::Fault(FaultStatus::from_bits_truncate(0xa4)))
    );
    trace.done();
}

#[test]
fn spi_errors_at_every_measurement_stage_attempt_cleanup() {
    for failed in [0, 1, 3, 5, 6] {
        let mut steps = one_shot(0x10, 11);
        match &mut steps[failed] {
            Step::Read(_, result) => *result = Err(ErrorKind::Other),
            Step::Write(_, result) => *result = Err(ErrorKind::Other),
            _ => unreachable!(),
        }
        steps.truncate(failed + 1);
        if failed != 0 && failed != 6 {
            steps.push(write(&[0x80, 0x10]));
        }
        let trace = Trace::new(steps);
        assert_eq!(
            run(sensor(&trace).read_rtd(&mut Delay(trace.clone()))),
            Err(Error::Spi(ErrorKind::Other))
        );
        trace.done();
    }
}

#[test]
fn original_error_wins_if_cleanup_also_fails() {
    let trace = Trace::new([
        read(0, &[0x10]),
        Step::Write(vec![0x80, 0x92], Err(ErrorKind::ModeFault)),
        Step::Write(vec![0x80, 0x10], Err(ErrorKind::ChipSelectFault)),
    ]);
    assert_eq!(
        run(sensor(&trace).read_rtd(&mut Delay(trace.clone()))),
        Err(Error::Spi(ErrorKind::ModeFault))
    );
    trace.done();
}

#[test]
fn status_read_error_still_turns_bias_off() {
    let mut steps = one_shot(0x10, 11);
    steps[5] = read(1, &[0x40, 1]);
    steps.insert(6, Step::Read(7, Err(ErrorKind::Overrun)));
    let trace = Trace::new(steps);
    assert_eq!(
        run(sensor(&trace).read_rtd(&mut Delay(trace.clone()))),
        Err(Error::Spi(ErrorKind::Overrun))
    );
    trace.done();
}

#[test]
fn fault_detection_commands_and_completion_use_device_state() {
    let trace = Trace::new([
        read(0, &[0xff]),
        write(&[0x80, 0x95]),
        read(0, &[0x95]),
        read(0, &[0x91]),
        read(7, &[0x34]),
        read(0, &[0xff]),
        write(&[0x80, 0x99]),
        read(0, &[0x99]),
        write(&[0x80, 0x9d]),
        read(0, &[0x91]),
    ]);
    let mut device = sensor(&trace);
    run(device.start_fault_cycle(FaultCycle::Auto)).unwrap();
    assert!(!run(device.fault_cycle_finished()).unwrap());
    assert!(run(device.fault_cycle_finished()).unwrap());
    assert_eq!(run(device.read_fault()).unwrap().bits(), 0x34);
    run(device.start_fault_cycle(FaultCycle::ManualRun)).unwrap();
    run(device.start_fault_cycle(FaultCycle::ManualFinish)).unwrap();
    assert!(run(device.fault_cycle_finished()).unwrap());
    trace.done();
}

#[test]
fn cancellation_after_trigger_requires_explicit_recovery() {
    let trace = Trace::new([
        read(0, &[0x10]),
        write(&[0x80, 0x92]),
        Step::Delay(11),
        write(&[0x80, 0xb0]),
    ]);
    let mut device = sensor(&trace);
    let mut delay = Delay(trace.clone());
    {
        let mut future = core::pin::pin!(device.read_rtd(&mut delay));
        let mut cx = Context::from_waker(Waker::noop());
        for _ in 0..5 {
            assert!(future.as_mut().poll(&mut cx).is_pending());
        }
        // Trigger reached hardware; conversion delay is suspended, then dropped.
        trace.done();
    }
    trace.0.borrow_mut().extend([
        write(&[0x80, 0x12]),
        write(&[0x83, 0xff, 0xfe, 0, 0]),
        Step::Delay(66),
    ]);
    trace.0.borrow_mut().extend(one_shot(0x10, 11));
    run(device.init()).unwrap();
    run(delay.delay_ms(66));
    assert_eq!(run(device.read_rtd(&mut delay)).unwrap(), 8192);
    trace.done();
}

#[test]
fn temperature_matches_independent_callendar_van_dusen_fixtures() {
    for nominal in [100.0_f64, 1000.0] {
        for t in [-200.0_f64, -100.0, -25.0, 0.0, 25.0, 100.0, 400.0, 850.0] {
            // Independent forward equation, including the IEC negative-temperature term.
            let ratio = 1.0 + 3.9083e-3 * t - 5.775e-7 * t * t
                + if t < 0.0 {
                    -4.183e-12 * (t - 100.0) * t * t * t
                } else {
                    0.0
                };
            let reference = nominal * 4.3;
            let raw = (32768.0 * nominal * ratio / reference).round() as u16;
            let actual = calculate_temperature(raw, nominal as f32, reference as f32).unwrap();
            assert!((actual - t as f32).abs() < 0.15, "{t}: {actual}");
        }
    }
    for (raw, nominal, reference) in [
        (0x8000, 100.0, 430.0),
        (1, 0.0, 430.0),
        (1, 100.0, f32::NAN),
        (1, f32::INFINITY, 430.0),
        (1, 100.0, -1.0),
        (32767, f32::MIN_POSITIVE, f32::MAX),
    ] {
        assert_eq!(calculate_temperature(raw, nominal, reference), None);
    }
}

#[test]
fn temperature_read_uses_the_configured_reference() {
    let trace = Trace::new(one_shot(0x10, 11));
    let cfg = Config {
        ref_resistor: RefResistor(400.0),
        ..Config::pt100_3wire()
    };
    let mut device = Max31865::new(Bus(trace.clone()), cfg).unwrap();
    let temperature = run(device.read_temperature(&mut Delay(trace.clone()))).unwrap();
    assert!(temperature.abs() < 0.01);
    trace.done();
}
