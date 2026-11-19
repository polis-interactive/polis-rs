/// Electrical DIR level and the sign used for pulse accounting.
/// Mechanical clockwise/counterclockwise mapping belongs to the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    /// DIR low; negative signed pulse count.
    Negative,
    /// DIR high; positive signed pulse count.
    Positive,
}

#[cfg(any(feature = "rp2040", feature = "rp235xa", test))]
impl Direction {
    pub(crate) fn bit(self) -> u32 {
        u32::from(self == Self::Positive)
    }

    pub(crate) fn signed(self, pulses: u16) -> i64 {
        match self {
            Self::Negative => -i64::from(pulses),
            Self::Positive => i64::from(pulses),
        }
    }
}

/// An atomic command. All timing is in configured PIO ticks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Segment {
    /// Emit a finite, nonzero number of rising STEP edges at constant period.
    /// The incoming period also determines spacing from the previous segment's
    /// final edge; changing period adds no incidental boundary pause.
    Pulses {
        /// Electrical direction, stable through every pulse in this segment.
        direction: Direction,
        /// Number of STEP rising edges. Zero is rejected.
        pulses: u16,
        /// Rising-edge spacing; includes all PIO overhead and the high pulse.
        period_ticks: u32,
    },
    /// Extend STEP low by this duration, retaining DIR. Split long low-speed
    /// intervals into these bounded delays to keep the commitment horizon short.
    Delay {
        /// Duration of this command, including its instruction overhead.
        ticks: u32,
    },
    /// Intentionally finish after previous commands. No later commands accepted
    /// until reset. Omitting this command makes queue exhaustion starvation.
    End,
}

/// Explicit electrical timing and maximum accepted future work. No motor defaults.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    /// PIO ticks/second. Must divide the injected system clock exactly; divider
    /// must be in 1..=65536. Keep that clock unchanged while the driver exists.
    pub tick_hz: u32,
    /// STEP high duration, 2..=17 ticks.
    pub step_high_ticks: u8,
    /// Minimum low time before changing DIR, 1..=16 ticks. Applied after every
    /// pulse, even when direction does not change. This is a falling-edge bound.
    pub dir_hold_ticks: u8,
    /// Minimum DIR-to-STEP-rising time. Periods too short to honor it are rejected.
    pub dir_setup_ticks: u16,
    /// Upper bound on accepted, not-yet-completed work, in ticks. Includes one
    /// conservative credit-return tick. Larger values increase response latency.
    pub max_committed_ticks: u32,
}

/// Configuration, encoding, or queue admission error. Rejected batches have no effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Zero tick rate, nonintegral divider, or divider outside hardware range.
    Clock,
    /// Invalid high/hold/setup timing or commitment budget.
    Timing,
    /// A pulse segment requested zero pulses; use End for an empty move.
    ZeroPulses,
    /// Period violates timing minima or the 15-bit delay range.
    Period,
    /// Delay is outside 15..=32781 ticks.
    Delay,
    /// At most four outstanding commands fit the completion ledger/FIFOs.
    QueueFull,
    /// Admission would exceed the configured time horizon.
    Horizon,
    /// End has already been accepted; reset after stopping before sending more.
    Closed,
    /// Driver has stopped; inspect its report and explicitly reset it.
    Stopped,
    /// Operation requires an idle/preloaded driver.
    Running,
    /// Start needs at least one committed command.
    EmptyQueue,
    /// Waiting for execution requires start to have been called.
    NotStarted,
    /// STEP and DIR must be distinct GPIOs in 0..=29.
    Pins,
    /// Fewer than 28 contiguous PIO instruction slots available.
    ProgramMemory,
}

impl Config {
    /// Check portable configuration, independent of a particular RP clock.
    pub fn validate(self) -> Result<(), Error> {
        if self.tick_hz == 0 {
            return Err(Error::Clock);
        }
        if !(2..=17).contains(&self.step_high_ticks)
            || !(1..=16).contains(&self.dir_hold_ticks)
            || self.dir_setup_ticks > 32774
            || self.max_committed_ticks <= self.min_period_ticks() + 17
        {
            return Err(Error::Timing);
        }
        Ok(())
    }

    /// Exact integer divider. No silent timing quantization or floating point.
    pub fn divider(self, system_hz: u32) -> Result<u32, Error> {
        self.validate()?;
        if !system_hz.is_multiple_of(self.tick_hz) {
            return Err(Error::Clock);
        }
        let divider = system_hz / self.tick_hz;
        if !(1..=65536).contains(&divider) {
            return Err(Error::Clock);
        }
        Ok(divider)
    }

    pub(crate) fn overhead(self) -> u32 {
        u32::from(self.step_high_ticks) + u32::from(self.dir_hold_ticks) + 16
    }

    /// Smallest accepted rising-edge period for this configuration.
    pub fn min_period_ticks(self) -> u32 {
        self.overhead() + u32::from(self.dir_setup_ticks).saturating_sub(7).max(1)
    }

    /// Largest representable period; the commitment budget can impose a tighter bound.
    pub fn max_period_ticks(self) -> u32 {
        self.overhead() + 32767
    }

    #[cfg(any(feature = "rp2040", feature = "rp235xa", test))]
    pub(crate) fn encode(self, segment: Segment, last_dir: Direction) -> Result<Entry, Error> {
        self.validate()?;
        let (delay, pulses, direction, ticks, end) = match segment {
            Segment::Pulses {
                direction,
                pulses,
                period_ticks,
            } => {
                if pulses == 0 {
                    return Err(Error::ZeroPulses);
                }
                if !(self.min_period_ticks()..=self.max_period_ticks()).contains(&period_ticks) {
                    return Err(Error::Period);
                }
                let ticks = u64::from(period_ticks) * u64::from(pulses);
                (
                    period_ticks - self.overhead(),
                    pulses,
                    direction,
                    ticks,
                    false,
                )
            }
            Segment::Delay { ticks } => {
                if !(15..=32781).contains(&ticks) {
                    return Err(Error::Delay);
                }
                (ticks - 14, 0, last_dir, u64::from(ticks), false)
            }
            // Nonzero sentinel, zero delay: checked before pulse generation.
            Segment::End => (0, 0, last_dir, 10, true),
        };
        if ticks + 1 > u64::from(self.max_committed_ticks) {
            return Err(Error::Horizon);
        }
        Ok(Entry {
            word: (delay << 17) | (direction.bit() << 16) | if end { 1 } else { u32::from(pulses) },
            ticks: ticks as u32,
            pulses,
            direction,
            end,
        })
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg(any(feature = "rp2040", feature = "rp235xa", test))]
pub(crate) struct Entry {
    pub word: u32,
    pub ticks: u32,
    pub pulses: u16,
    pub direction: Direction,
    pub end: bool,
}

/// Completed-segment checkpoint, NOT a live position measurement.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Progress {
    /// STEP rising edges in acknowledged segments, modulo 2^64.
    pub completed_pulses: u64,
    /// Signed STEP rising edges in acknowledged segments, wrapping i64.
    pub completed_signed_pulses: i64,
    /// Commands accepted but not yet acknowledged (includes a pending End).
    pub outstanding_commands: usize,
    /// Conservative remaining commitment, including the current whole segment.
    /// Does not measure elapsed time or include application/bus scheduling latency.
    pub committed_ticks: u32,
}

/// Why execution stopped. Stopped engines require explicit reset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopReason {
    /// The explicit End command was reached with STEP low.
    Completed,
    /// PIO exhausted its input. Late data cannot restart execution.
    Starved,
    /// Caller requested an immediate stop.
    Aborted,
    /// An in-flight commit or wait future was dropped.
    Cancelled,
}

/// Execution at the instant the SM was disabled, including a partial segment.
/// These are generated electrical edges, not proof the motor followed them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StopReport {
    /// Stop cause.
    pub reason: StopReason,
    /// Total STEP rising edges this run, modulo 2^64.
    pub emitted_pulses: u64,
    /// Signed total this run, wrapping i64.
    pub emitted_signed_pulses: i64,
    /// Conservative indication that abort may have shortened the last high pulse.
    /// Its rising edge is included, but acceptance by the external driver is uncertain.
    pub pulse_may_be_truncated: bool,
}
