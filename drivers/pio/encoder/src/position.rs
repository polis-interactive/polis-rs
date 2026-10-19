//! Portable policy. No GPIO, timing, allocation, or runtime dependencies.

/// Both inputs, encoded as `(A << 1) | B` (B is the lower GPIO).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Phase {
    /// A low, B low.
    LowLow = 0,
    /// A low, B high.
    LowHigh = 1,
    /// A high, B low.
    HighLow = 2,
    /// A high, B high.
    HighHigh = 3,
}

impl Phase {
    /// Construct from electrical levels, B first, A second.
    pub const fn from_b_a(b: bool, a: bool) -> Self {
        match (a, b) {
            (false, false) => Self::LowLow,
            (false, true) => Self::LowHigh,
            (true, false) => Self::HighLow,
            (true, true) => Self::HighHigh,
        }
    }
}

/// Wrapping 32-bit electrical transition count, positive for A leading B:
/// `00 -> 10 -> 11 -> 01 -> 00`. One complete cycle is four transitions.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Position(pub u32);

impl Position {
    /// Signed modular difference. Correct only when net displacement is strictly
    /// between -2^31 and +2^31 transitions. No detection of missed revolutions.
    pub const fn wrapping_delta_since(self, previous: Self) -> i32 {
        self.0.wrapping_sub(previous.0) as i32
    }
}

/// Reference decoder for sampled phases. A repeated phase does nothing; an
/// adjacent phase adds +/-1; a two-bit jump adds zero and resynchronizes phase.
/// Invalid jumps make physical position uncertain; resynchronization cannot
/// recover the missing travel. First observation establishes zero at any phase.
#[derive(Clone, Debug, Default)]
pub struct Decoder {
    previous: Option<Phase>,
    position: Position,
    invalid: u32,
}

impl Decoder {
    /// Sample one phase and return the cumulative wrapping position.
    pub fn sample(&mut self, phase: Phase) -> Position {
        if let Some(previous) = self.previous {
            let delta: i32 = match (previous as u8, phase as u8) {
                (0, 2) | (2, 3) | (3, 1) | (1, 0) => 1,
                (2, 0) | (3, 2) | (1, 3) | (0, 1) => -1,
                (a, b) if a ^ b == 3 => {
                    self.invalid = self.invalid.wrapping_add(1);
                    0
                }
                _ => 0,
            };
            self.position.0 = self.position.0.wrapping_add_signed(delta);
        }
        self.previous = Some(phase);
        self.position
    }

    /// Current cumulative position, initially zero.
    pub const fn position(&self) -> Position {
        self.position
    }

    /// Observed two-bit jumps, modulo 2^32. This diagnostic exists only in the
    /// portable decoder: the PIO counter does **not** export invalid-jump counts.
    /// Even this count does not detect transitions skipped between samples.
    pub const fn invalid_transitions(&self) -> u32 {
        self.invalid
    }
}

/// Application's positive direction. Clockwise depends on wiring and mechanism.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    /// Positive for `00 -> 10 -> 11 -> 01 -> 00` in AB notation.
    ALeadsB,
    /// Invert the electrical convention.
    BLeadsA,
}

/// Units of net travel from the baseline, **not phase-anchored detent events**.
/// Consult the encoder's transitions-per-detent specification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resolution {
    /// One unit per electrical transition.
    Transition,
    /// One unit per two net transitions.
    HalfStep,
    /// One unit per four net transitions.
    FullStep,
}

impl Resolution {
    const fn divisor(self) -> i64 {
        match self {
            Self::Transition => 1,
            Self::HalfStep => 2,
            Self::FullStep => 4,
        }
    }
}

/// Extended position at one snapshot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Reading {
    /// Signed electrical travel since the tracker's baseline, after inversion.
    pub transitions: i64,
    /// Net travel divided by the resolution, rounded toward zero.
    pub position: i64,
    /// Change in reported position since the last successful update.
    pub delta: i64,
}

/// Detectable tracking failures. Other violations of the half-range assumption
/// cannot be detected from a modulo counter; no exact loss detection is claimed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackError {
    /// Exactly 2^31 transitions: the sign is ambiguous.
    AmbiguousDelta,
    /// The extended signed 64-bit electrical position would overflow.
    PositionOverflow,
}

/// Extend hardware snapshots with a fixed baseline and configurable units.
/// Keep this value alive across reconnects. Updates need net displacement <2^31
/// transitions; instantaneous edge processing is unnecessary. Reporting is
/// path-independent, so delayed consumption produces the same final position.
#[derive(Clone, Debug)]
pub struct Tracker {
    previous: Position,
    transitions: i64,
    resolution: Resolution,
    direction: Direction,
}

impl Tracker {
    /// Establish a baseline without movement. Use `Position(0)` to retain travel
    /// since PIO startup; use a fresh snapshot to deliberately establish a new zero.
    pub const fn new(baseline: Position, resolution: Resolution, direction: Direction) -> Self {
        Self {
            previous: baseline,
            transitions: 0,
            resolution,
            direction,
        }
    }

    /// Extend a snapshot. Errors leave all state unchanged. To recover after a
    /// known outage beyond the half-range bound, explicitly construct a new
    /// tracker from a fresh snapshot; the lost number of wraps is unrecoverable.
    pub fn update(&mut self, raw: Position) -> Result<Reading, TrackError> {
        let delta = raw.wrapping_delta_since(self.previous);
        if delta == i32::MIN {
            return Err(TrackError::AmbiguousDelta);
        }
        let delta = match self.direction {
            Direction::ALeadsB => i64::from(delta),
            Direction::BLeadsA => -i64::from(delta),
        };
        let transitions = self
            .transitions
            .checked_add(delta)
            .ok_or(TrackError::PositionOverflow)?;
        let position = transitions / self.resolution.divisor();
        let previous = self.position();
        self.previous = raw;
        self.transitions = transitions;
        Ok(Reading {
            transitions,
            position,
            delta: position - previous,
        })
    }

    /// Last successfully observed position in configured units.
    pub const fn position(&self) -> i64 {
        self.transitions / self.resolution.divisor()
    }

    /// Last successfully observed electrical travel, after direction inversion.
    pub const fn transitions(&self) -> i64 {
        self.transitions
    }
}

#[cfg(test)]
mod tests;
