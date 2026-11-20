use super::*;
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use embedded_hal_async::spi::{ErrorKind, ErrorType, Operation};
use registers::*;
use std::{cell::RefCell, rc::Rc, vec::Vec};

#[derive(Clone, Debug, PartialEq)]
enum Event {
    Delay(u32),
    Frame([u8; 5]),
    Deselect,
}
struct State {
    registers: [u32; 128],
    pending_read: u32,
    events: Vec<Event>,
    transfers: usize,
    cs_low: bool,
    failure: Option<(usize, bool)>,
}
type Shared = Rc<RefCell<State>>;
struct Spi(Shared);
struct Delay(Shared);

fn device() -> (Tmc5160<Spi, Delay>, Shared) {
    let mut registers = [0; 128];
    registers[4] = 0x3000_0050;
    let state = Rc::new(RefCell::new(State {
        registers,
        pending_read: 0xdead_beef,
        events: Vec::new(),
        transfers: 0,
        cs_low: false,
        failure: None,
    }));
    (
        Tmc5160::new(
            Spi(state.clone()),
            Delay(state.clone()),
            BoardConfig::BTT_TMC5160T_PLUS,
            Timing::INTERNAL_CLOCK,
        ),
        state,
    )
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
    .await;
}

impl DelayNs for Delay {
    async fn delay_ns(&mut self, ns: u32) {
        {
            let mut state = self.0.borrow_mut();
            assert!(!state.cs_low, "inter-datagram delay must be with CS high");
            state.events.push(Event::Delay(ns));
        }
        suspend().await;
    }
}
impl ErrorType for Spi {
    type Error = ErrorKind;
}
impl SpiDevice<u8> for Spi {
    async fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), ErrorKind> {
        let [Operation::TransferInPlace(bytes)] = operations else {
            panic!("one full-duplex datagram per CS assertion");
        };
        let tx: [u8; 5] = bytes[..].try_into().expect("40-bit datagram");
        let (index, failure) = {
            let mut state = self.0.borrow_mut();
            assert!(!state.cs_low);
            state.cs_low = true;
            state.events.push(Event::Frame(tx));
            let index = state.transfers;
            state.transfers += 1;
            (index, state.failure.filter(|f| f.0 == index))
        };
        suspend().await; // Pending backend can leave CS asserted when dropped.
        if failure == Some((index, false)) {
            self.0.borrow_mut().cs_low = false;
            return Err(ErrorKind::ModeFault);
        }
        {
            let mut state = self.0.borrow_mut();
            bytes[0] = 0x10 + (index as u8 % 16);
            bytes[1..].copy_from_slice(&state.pending_read.to_be_bytes());
            let address = usize::from(tx[0] & 0x7f);
            if tx[0] & 0x80 != 0 {
                let value = u32::from_be_bytes(tx[1..].try_into().unwrap());
                if address == 1 {
                    state.registers[1] &= !value;
                } else {
                    state.registers[address] = value;
                }
            } else {
                state.pending_read = state.registers[address];
            }
        }
        suspend().await; // Hardware may have changed before completion/error.
        let mut state = self.0.borrow_mut();
        state.cs_low = false;
        state.events.push(Event::Deselect);
        if failure == Some((index, true)) {
            return Err(ErrorKind::ModeFault);
        }
        Ok(())
    }
}

fn run<F: Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    for _ in 0..1000 {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
    }
    panic!("mock did not finish");
}
fn cancel_after<F: Future>(future: F, pending_count: usize) {
    let mut future = core::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    for _ in 0..pending_count {
        assert!(future.as_mut().poll(&mut context).is_pending());
    }
}
fn frames(state: &Shared) -> Vec<[u8; 5]> {
    state
        .borrow()
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Frame(f) => Some(*f),
            _ => None,
        })
        .collect()
}
fn config() -> StepDirConfig {
    let current = BoardConfig::BTT_TMC5160T_PLUS
        .current_settings(800, 400, 4)
        .unwrap();
    StepDirConfig::new(
        current,
        Chopconf::DISABLED
            .with_microsteps(Microsteps::M16, true)
            .with_off_time(3)
            .unwrap(),
    )
}
async fn repair(spi: &mut Spi) -> Result<(), ErrorKind> {
    suspend().await;
    spi.0.borrow_mut().cs_low = false;
    suspend().await;
    Ok(())
}

#[test]
fn spi_pipeline_byte_order_status_and_cs_boundaries() {
    let (mut driver, state) = device();
    state.borrow_mut().registers[0x6f] = 0x8123_4567;
    let reply = run(driver.read::<DrvStatus>()).unwrap();
    assert_eq!(reply.value.bits(), 0x8123_4567);
    assert_eq!(reply.request_status.bits(), 0x10);
    assert_eq!(reply.response_status.bits(), 0x11);
    assert_eq!(frames(&state), [[0x6f, 0, 0, 0, 0], [0x6f, 0, 0, 0, 0]]);
    assert_eq!(run(driver.write(Pwmconf::RESET)).unwrap().bits(), 0x12);
    assert_eq!(frames(&state)[2], [0xf0, 0xc4, 0x0c, 0, 0x1e]);
    for chunk in state.borrow().events.as_chunks::<3>().0 {
        assert_eq!(chunk[0], Event::Delay(1000));
        assert!(matches!(chunk[1], Event::Frame(_)));
        assert_eq!(chunk[2], Event::Deselect);
    }
    // A subsequent read must not consume the stale DRV_STATUS pipeline response.
    assert_eq!(
        run(driver.read::<Ioin>()).unwrap().value.bits(),
        0x3000_0050
    );
}

#[test]
fn errors_before_and_after_each_read_transfer_require_recovery() {
    for index in 0..2 {
        for after_effect in [false, true] {
            let (mut driver, state) = device();
            state.borrow_mut().failure = Some((index, after_effect));
            assert_eq!(
                run(driver.read::<Ioin>()),
                Err(Error::Spi(ErrorKind::ModeFault))
            );
            let count = state.borrow().transfers;
            assert_eq!(run(driver.read::<Ioin>()), Err(Error::RecoveryRequired));
            assert_eq!(state.borrow().transfers, count);
            state.borrow_mut().failure = None;
            run(driver.recover_with(repair)).unwrap();
            assert_eq!(run(driver.read::<Ioin>()).unwrap().value.version(), 0x30);
        }
    }
}

#[test]
fn cancellation_at_every_read_await_blocks_io_until_explicit_repair() {
    for pending in 1..=6 {
        let (mut driver, state) = device();
        cancel_after(driver.read::<Ioin>(), pending);
        assert!(driver.recovery_required());
        let count = state.borrow().transfers;
        assert_eq!(run(driver.read::<Ioin>()), Err(Error::RecoveryRequired));
        assert_eq!(state.borrow().transfers, count);
        run(driver.recover_with(repair)).unwrap();
        assert!(!driver.recovery_required());
        assert!(!state.borrow().cs_low);
        assert_eq!(run(driver.read::<Ioin>()).unwrap().value.version(), 0x30);
    }
}

#[test]
fn cancelled_or_failed_recovery_stays_blocked() {
    for pending in 1..=3 {
        let (mut driver, _) = device();
        cancel_after(driver.write(Pwmconf::RESET), 2);
        cancel_after(driver.recover_with(repair), pending);
        assert!(driver.recovery_required());
        run(driver.recover_with(repair)).unwrap();
        assert!(!driver.recovery_required());
    }
    let (mut driver, _) = device();
    assert_eq!(
        run(driver.recover_with(async |_| Err(ErrorKind::Overrun))),
        Err(Error::Spi(ErrorKind::Overrun))
    );
    assert!(driver.recovery_required());
}

#[test]
fn cancelled_writes_are_not_replayed() {
    for pending in 1..=3 {
        let (mut driver, state) = device();
        cancel_after(driver.write(Pwmconf::RESET), pending);
        let before = frames(&state);
        assert!(driver.recovery_required());
        assert_eq!(
            run(driver.write(Pwmconf::RESET)),
            Err(Error::RecoveryRequired)
        );
        run(driver.recover_with(repair)).unwrap();
        assert_eq!(frames(&state), before);
        assert_eq!(
            state.borrow().registers[0x70],
            if pending == 3 {
                Pwmconf::RESET.bits()
            } else {
                0
            }
        );
    }
}

#[test]
fn configure_checks_hardware_before_writes_and_applies_chopper_last() {
    for (input, error) in [
        (0, Error::UnexpectedVersion(0)),
        (u32::MAX, Error::UnexpectedVersion(255)),
        (0x3000_0010, Error::ExternalStepDirRequired),
        (0x3000_0040, Error::DriverDisableRequired),
    ] {
        let (mut driver, state) = device();
        state.borrow_mut().registers[4] = input;
        assert_eq!(run(driver.configure(&config())), Err(error));
        assert_eq!(frames(&state).len(), 2);
        assert!(frames(&state).iter().all(|f| f[0] & 0x80 == 0));
    }
    let (mut driver, state) = device();
    run(driver.configure(&config())).unwrap();
    let sent = frames(&state);
    assert_eq!(
        sent.iter().map(|f| f[0]).collect::<Vec<_>>(),
        [
            4, 4, 0xec, 0x8b, 0x90, 0x91, 0x93, 0x94, 0x95, 0xed, 0x80, 0xf0, 0xf0, 0x80, 0xec
        ]
    );
    assert_eq!(sent[2], [0xec, 0x14, 0x41, 1, 0x50]);
    assert_eq!(sent[14], [0xec, 0x14, 0x41, 1, 0x53]);
    assert_eq!(
        driver.configuration_progress(),
        ConfigurationProgress::Complete
    );
    assert_eq!(state.borrow().registers[4] & 0x10, 0x10);
}

#[test]
fn every_config_write_failure_reports_partial_progress_without_retry() {
    for index in 2..15 {
        for after_effect in [false, true] {
            let (mut driver, state) = device();
            state.borrow_mut().failure = Some((index, after_effect));
            assert_eq!(
                run(driver.configure(&config())),
                Err(Error::Spi(ErrorKind::ModeFault))
            );
            assert_eq!(state.borrow().transfers, index + 1);
            assert_eq!(
                driver.configuration_progress(),
                ConfigurationProgress::Writing {
                    completed: (index - 2) as u8,
                    address: frames(&state)[index][0] & 0x7f,
                }
            );
            assert!(driver.recovery_required());
        }
    }
}

#[test]
fn cancellation_at_every_config_await_preserves_progress() {
    for pending in 1..=45 {
        let (mut driver, state) = device();
        cancel_after(driver.configure(&config()), pending);
        assert!(driver.recovery_required());
        if pending <= 6 {
            assert_eq!(
                driver.configuration_progress(),
                ConfigurationProgress::Checking
            );
        } else {
            let ConfigurationProgress::Writing { completed, .. } = driver.configuration_progress()
            else {
                panic!();
            };
            assert_eq!(usize::from(completed), (pending - 7) / 3);
        }
        let before = frames(&state);
        run(driver.recover_with(repair)).unwrap();
        assert_eq!(frames(&state), before);
        assert_eq!(
            driver.configuration_progress(),
            ConfigurationProgress::Unknown
        );
    }
}

#[test]
fn explicit_bridge_registers_and_status_clear_are_not_implicit_defaults() {
    let (mut driver, state) = device();
    let mut conf = config();
    conf.short = Some(ShortConf::from_bits(0x0001_0606).unwrap());
    conf.bridge = Some(DrvConf::from_bits(0x0008_0400).unwrap());
    run(driver.configure(&conf)).unwrap();
    assert_eq!(frames(&state)[3], [0x89, 0, 1, 6, 6]);
    assert_eq!(frames(&state)[4], [0x8a, 0, 8, 4, 0]);
    state.borrow_mut().registers[1] = 7;
    assert_eq!(run(driver.read::<Gstat>()).unwrap().value.bits(), 7);
    run(driver.clear_status(Gstat::clear_flags(true, false, true))).unwrap();
    assert_eq!(state.borrow().registers[1], 2);
    assert_eq!(frames(&state).last(), Some(&[0x81, 0, 0, 0, 5]));
}

#[test]
fn validation_rejects_reserved_bits_invalid_fields_and_board_mismatch_before_io() {
    assert!(Gconf::from_bits(1 << 16).is_err()); // direct coil-current mode excluded
    assert!(Chopconf::from_bits(1 << 17).is_err());
    assert!(Chopconf::from_bits(1 << 13).is_err());
    assert!(Chopconf::from_bits(9 << 24).is_err());
    assert!(Chopconf::from_bits(1).is_err()); // TOFF=1 with short blanking
    assert!(Chopconf::from_bits((15 << 7) | (7 << 4)).is_err());
    assert!(Pwmconf::from_bits(1 << 23).is_err());
    assert!(Pwmconf::from_bits(1 << 19).is_err());
    assert!(Coolconf::from_bits(1 << 23).is_err());
    assert!(ShortConf::from_bits(0x103).is_err());
    assert!(DrvConf::from_bits(25).is_err());
    assert!(Tpwmthrs::from_bits(1 << 20).is_err());
    for value in 1..32 {
        assert!(GlobalScaler::from_bits(value).is_err());
    }
    assert_eq!(GlobalScaler::new(256).unwrap().bits(), 0);
    assert_eq!(GlobalScaler::from_bits(0).unwrap().scale(), 256);
    let (mut driver, state) = device();
    let mut conf = config();
    conf.global = Gconf::from_read(1 << 17);
    assert!(matches!(
        run(driver.configure(&conf)),
        Err(Error::Config(_))
    ));
    assert!(matches!(
        run(driver.write(conf.global)),
        Err(Error::Config(_))
    ));
    conf = config();
    conf.current = BoardConfig::new(75_000, 3000)
        .unwrap()
        .current_settings(800, 400, 4)
        .unwrap();
    assert_eq!(
        run(driver.configure(&conf)),
        Err(Error::Config(ConfigError::BoardMismatch))
    );
    assert!(state.borrow().events.is_empty());
}

#[test]
fn btt_current_math_matches_datasheet_formula_and_never_rounds_up() {
    let board = BoardConfig::BTT_TMC5160T_PLUS;
    assert_eq!(board.sense_microohms(), 22_000);
    assert_eq!(board.max_rms_milliamps(), 10_600);
    assert!((board.full_scale_rms_milliamps() - 10_445.895).abs() < 0.01);
    let settings = board.current_settings(800, 400, 4).unwrap();
    assert_eq!(settings.global_scaler().scale(), 33);
    assert_eq!(settings.ihold_irun().run(), 18);
    assert_eq!(settings.ihold_irun().hold(), 8);
    let independently_calculated =
        0.325 / 0.022 / core::f64::consts::SQRT_2 * 1000.0 * 33.0 / 256.0;
    assert!((settings.run_rms_milliamps() - independently_calculated * 19.0 / 32.0).abs() < 1e-9);
    for run_ma in (100..10_445).step_by(37) {
        let settings = board.current_settings(run_ma, run_ma / 2, 15).unwrap();
        assert!(settings.run_rms_milliamps() <= f64::from(run_ma));
        assert!(settings.hold_rms_milliamps() <= f64::from(run_ma / 2));
        assert!((32..=256).contains(&settings.global_scaler().scale()));
        assert!(settings.ihold_irun().hold() <= settings.ihold_irun().run());
    }
    for (run, hold, delay) in [
        (0, 0, 0),
        (800, 0, 0),
        (800, 801, 0),
        (800, 400, 16),
        (11_000, 500, 0),
        (10_600, 500, 0),
        (1, 1, 0),
    ] {
        assert_eq!(
            board.current_settings(run, hold, delay),
            Err(ConfigError::Current)
        );
    }
    assert!(BoardConfig::new(0, 1000).is_err());
    assert!(BoardConfig::new(22_000, 0).is_err());
    assert!(Timing::new(0).is_err());
}

#[test]
fn diagnostic_field_decoding_and_signed_stall_threshold() {
    let status = DrvStatus::from_read(0xff1f_f3ff);
    assert_eq!(status.stall_guard_result(), 1023);
    assert_eq!(status.current_scale(), 31);
    assert!(status.standstill() && status.stealth_chop() && status.stalled());
    assert!(status.overtemperature() && status.temperature_warning());
    assert_eq!(status.short_to_ground(), 3);
    assert_eq!(status.short_to_supply(), 3);
    assert_eq!(status.open_load(), 3);
    assert_eq!(
        Coolconf::DISABLED
            .with_stall_guard(-64, true)
            .unwrap()
            .bits(),
        0x0140_0000
    );
    assert_eq!(
        Coolconf::DISABLED
            .with_stall_guard(63, false)
            .unwrap()
            .bits(),
        0x003f_0000
    );
    assert!(Coolconf::DISABLED.with_stall_guard(-65, false).is_err());
    let spi = SpiStatus(0x0f);
    assert!(spi.reset() && spi.driver_error() && spi.stalled() && spi.standstill());
}

#[test]
fn automatic_pwm_is_seeded_before_stealthchop_even_on_reconfiguration() {
    let (mut driver, state) = device();
    let mut conf = config();
    conf.global = conf.global.with_stealth_chop(true);
    conf.pwm = Pwmconf::from_bits((conf.pwm.bits() & !0xffff) | (23 << 8) | 58).unwrap();
    state.borrow_mut().registers[0] = conf.global.bits();
    run(driver.configure(&conf)).unwrap();
    let sent = frames(&state);
    let value = |i: usize| u32::from_be_bytes(sent[i][1..5].try_into().unwrap());
    assert_eq!(sent[10][0], 0x80);
    assert_eq!(value(10) & 4, 0);
    assert_eq!(sent[11][0], 0xf0);
    assert_eq!(value(11) & ((1 << 18) | (1 << 19)), 0);
    assert_eq!(value(11) & 0xffff, (23 << 8) | 58);
    assert_eq!(sent[12][0], 0xf0);
    assert_eq!(value(12), conf.pwm.bits());
    assert_eq!(sent[13][0], 0x80);
    assert_eq!(value(13), conf.global.bits());
    assert_eq!(sent[14][0], 0xec);
}
