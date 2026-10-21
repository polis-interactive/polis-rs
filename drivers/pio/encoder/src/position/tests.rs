use super::*;

fn phase(bits: u8) -> Phase {
    Phase::from_b_a(bits & 1 != 0, bits & 2 != 0)
}

#[test]
fn every_startup_phase_establishes_zero() {
    for bits in 0..4 {
        let mut decoder = Decoder::default();
        for _ in 0..10 {
            assert_eq!(decoder.sample(phase(bits)), Position(0));
        }
        assert_eq!(decoder.invalid_transitions(), 0);
    }
}

#[test]
fn clockwise_counterclockwise_all_rotated_start_phases() {
    // "Clockwise" here is a fixture convention: A leads B.
    let cycle = [0, 2, 3, 1];
    for start in 0..4 {
        for sign in [-1i32, 1] {
            let mut d = Decoder::default();
            for step in 0..=24 {
                let index = (start + sign * step).rem_euclid(4) as usize;
                assert_eq!(d.sample(phase(cycle[index])).0, (sign * step) as u32);
            }
        }
    }
}

#[test]
fn bounce_and_mid_detent_reversal_cancel_net_travel() {
    for sequence in [&[0, 2, 0, 2, 0][..], &[0, 2, 3, 2, 0], &[3, 1, 3, 1, 3]] {
        let mut d = Decoder::default();
        for &p in sequence {
            d.sample(phase(p));
        }
        assert_eq!(d.position(), Position(0));
        assert_eq!(d.invalid_transitions(), 0);
    }
    let mut d = Decoder::default();
    for p in [0, 2, 0, 2, 3, 2, 3, 1, 3, 1, 0, 1, 0] {
        d.sample(phase(p));
    }
    assert_eq!(d.position(), Position(4));
}

#[test]
fn invalid_jumps_add_zero_resynchronize_and_count_only_observed_errors() {
    for start in 0..4 {
        let mut d = Decoder::default();
        d.sample(phase(start));
        assert_eq!(d.sample(phase(start ^ 3)), Position(0));
        assert_eq!(d.sample(phase(start)), Position(0));
        assert_eq!(d.invalid_transitions(), 2);
        // Following legal edge is interpreted relative to the new phase.
        let next = [2, 0, 3, 1][start as usize];
        assert_eq!(d.sample(phase(next)), Position(1));
    }
}

#[test]
fn full_half_transition_units_round_toward_zero_and_invert() {
    for (resolution, divisor) in [
        (Resolution::FullStep, 4),
        (Resolution::HalfStep, 2),
        (Resolution::Transition, 1),
    ] {
        for direction in [Direction::ALeadsB, Direction::BLeadsA] {
            let sign = if direction == Direction::ALeadsB {
                1
            } else {
                -1
            };
            let mut t = Tracker::new(Position(0), resolution, direction);
            let mut previous = 0;
            for raw in [0i64, 1, 2, 3, 4, 3, 4, 8, 6, 1, 0, -1, -2, -3, -4, -8, 0] {
                let r = t.update(Position(raw as u32)).unwrap();
                assert_eq!(r.transitions, raw * sign);
                assert_eq!(r.position, raw * sign / divisor);
                assert_eq!(r.delta, r.position - previous);
                assert_eq!(t.position(), r.position);
                previous = r.position;
            }
        }
    }
}

#[test]
fn delayed_consumption_preserves_final_position_even_across_reversals() {
    for resolution in [
        Resolution::FullStep,
        Resolution::HalfStep,
        Resolution::Transition,
    ] {
        let mut eager = Tracker::new(Position(0), resolution, Direction::ALeadsB);
        let mut delayed = eager.clone();
        for raw in [0, 3, 4, 3, 2, 9, 6, 10004, 9999] {
            eager.update(Position(raw)).unwrap();
        }
        let reading = delayed.update(Position(9999)).unwrap();
        assert_eq!(eager.position(), reading.position);
        assert_eq!(eager.transitions(), reading.transitions);
    }
}

#[test]
fn baseline_is_explicit_and_modular_wrap_is_extended_in_both_directions() {
    let mut t = Tracker::new(
        Position(u32::MAX - 1),
        Resolution::Transition,
        Direction::ALeadsB,
    );
    assert_eq!(t.update(Position(2)).unwrap().position, 4);
    assert_eq!(t.update(Position(u32::MAX - 2)).unwrap().position, -1);
    let mut d = Decoder::default();
    d.sample(phase(0));
    assert_eq!(d.sample(phase(1)), Position(u32::MAX));
    assert_eq!(d.sample(phase(0)), Position(0));
}

#[test]
fn ambiguous_half_range_and_i64_overflow_leave_state_unchanged() {
    let mut t = Tracker::new(Position(0), Resolution::Transition, Direction::ALeadsB);
    assert_eq!(t.update(Position(1 << 31)), Err(TrackError::AmbiguousDelta));
    assert_eq!(t.update(Position(1)).unwrap().position, 1);
    t.transitions = i64::MAX;
    assert_eq!(t.update(Position(2)), Err(TrackError::PositionOverflow));
    assert_eq!(t.previous, Position(1));
    t.transitions = i64::MIN;
    assert_eq!(t.update(Position(0)), Err(TrackError::PositionOverflow));
    assert_eq!(t.previous, Position(1));
}
