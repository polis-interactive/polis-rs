use super::*;
use crate::{Config, Segment};

fn config() -> Config {
    Config {
        tick_hz: 4_000_000,
        step_high_ticks: 4,
        dir_hold_ticks: 4,
        dir_setup_ticks: 4,
        max_committed_ticks: 8000,
    }
}

fn pulses(n: u16, period: u32) -> Segment {
    Segment::Pulses {
        direction: Direction::Positive,
        pulses: n,
        period_ticks: period,
    }
}

#[test]
fn timing_rejects_zero_overflow_and_unrepresentable_clocks() {
    let c = config();
    assert_eq!(c.divider(128_000_000), Ok(32));
    assert_eq!(c.divider(125_000_000), Err(Error::Clock));
    assert_eq!(c.divider(0), Err(Error::Clock));
    assert_eq!(
        c.encode(pulses(0, 80), Direction::Negative).unwrap_err(),
        Error::ZeroPulses
    );
    assert_eq!(
        c.encode(pulses(1, 24), Direction::Negative).unwrap_err(),
        Error::Period
    );
    assert_eq!(
        c.encode(pulses(1, u32::MAX), Direction::Negative)
            .unwrap_err(),
        Error::Period
    );
    assert_eq!(
        c.encode(pulses(u16::MAX, 32000), Direction::Negative)
            .unwrap_err(),
        Error::Horizon
    );
    assert_eq!(Config { tick_hz: 0, ..c }.validate(), Err(Error::Clock));
    assert_eq!(
        Config {
            step_high_ticks: 18,
            ..c
        }
        .validate(),
        Err(Error::Timing)
    );
    assert_eq!(
        Config {
            dir_hold_ticks: 17,
            ..c
        }
        .validate(),
        Err(Error::Timing)
    );
    assert_eq!(
        Config {
            dir_setup_ticks: u16::MAX,
            ..c
        }
        .validate(),
        Err(Error::Timing)
    );
    let setup = Config {
        dir_setup_ticks: 30,
        ..c
    };
    assert_eq!(setup.min_period_ticks(), 47);
    assert_eq!(
        setup
            .encode(pulses(1, 46), Direction::Negative)
            .unwrap_err(),
        Error::Period
    );
    assert!(setup.encode(pulses(1, 47), Direction::Negative).is_ok());
}

#[test]
fn horizon_accounts_for_startup_tail_and_entire_active_segment() {
    let c = config();
    let mut q = Queue::new(Direction::Negative);
    let e = c.encode(pulses(40, 80), q.direction).unwrap();
    q.push(e, 8000).unwrap();
    q.push(e, 8000).unwrap();
    assert_eq!(q.progress().committed_ticks, 6418);
    assert_eq!(q.push(e, 8000), Err(Error::Horizon));
    assert_eq!(q.progress().outstanding_commands, 2);
    q.acknowledge();
    assert_eq!(q.progress().committed_ticks, 3201);
    q.push(e, 8000).unwrap();
    assert_eq!(q.progress().committed_ticks, 6401);
}

#[test]
fn four_records_bound_both_fifos_and_end_closes_admission() {
    let c = config();
    let e = c.encode(pulses(1, 80), Direction::Positive).unwrap();
    let mut q = Queue::new(Direction::Negative);
    for _ in 0..4 {
        q.push(e, 8000).unwrap();
    }
    assert_eq!(q.push(e, 8000), Err(Error::QueueFull));
    for _ in 0..4 {
        q.acknowledge();
    }
    q.push(c.encode(Segment::End, q.direction).unwrap(), 8000)
        .unwrap();
    assert_eq!(q.push(e, 8000), Err(Error::Closed));
    assert_eq!(q.stopped(24, 1, StopReason::Completed).emitted_pulses, 4);
}

#[test]
fn counts_wrap_explicitly_and_partial_negative_counts_are_signed() {
    let c = config();
    let mut q = Queue::new(Direction::Negative);
    q.pulses = u64::MAX;
    q.signed = i64::MAX;
    q.push(c.encode(pulses(1, 80), Direction::Positive).unwrap(), 8000)
        .unwrap();
    q.acknowledge();
    assert_eq!(q.progress().completed_pulses, 0);
    assert_eq!(q.progress().completed_signed_pulses, i64::MIN);
    q.push(
        c.encode(
            Segment::Pulses {
                direction: Direction::Negative,
                pulses: 5,
                period_ticks: 80,
            },
            Direction::Negative,
        )
        .unwrap(),
        8000,
    )
    .unwrap();
    let report = q.stopped(13, 3, StopReason::Aborted);
    assert_eq!(report.emitted_pulses, 2);
    assert_eq!(report.emitted_signed_pulses, i64::MAX - 1);
    assert!(report.pulse_may_be_truncated);
}

#[test]
fn delays_and_end_are_nonzero_words_without_pulses() {
    let c = config();
    assert_eq!(
        c.encode(Segment::Delay { ticks: 14 }, Direction::Negative)
            .unwrap_err(),
        Error::Delay
    );
    for segment in [Segment::Delay { ticks: 15 }, Segment::End] {
        let e = c.encode(segment, Direction::Negative).unwrap();
        assert_ne!(e.word, 0);
        assert_eq!(e.pulses, 0);
    }
}

#[test]
fn batch_admission_is_atomic_including_end_and_invalid_tail() {
    let c = config();
    let q = Queue::new(Direction::Negative);
    let before = q.progress();
    for segments in [
        [pulses(1, 80), pulses(0, 80)],
        [Segment::End, pulses(1, 80)],
        [pulses(40, 80), pulses(80, 80)],
    ] {
        assert!(q.prepare(c, &segments).is_err());
        assert_eq!(q.progress(), before);
        assert!(!q.closed);
    }
    let (accepted, words) = q.prepare(c, &[pulses(1, 80), Segment::End]).unwrap();
    assert_eq!(accepted.progress().outstanding_commands, 2);
    assert!(accepted.closed);
    assert!(words[..2].iter().all(|&word| word != 0));
    assert_eq!(q.progress(), before);
}
