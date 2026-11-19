use crate::{Config, Direction, Error, Progress, Segment, StopReason, StopReport, timing::Entry};

// Four outstanding commands bound BOTH TX occupancy and completion RX occupancy.
// The ledger is updated before DMA starts; cancellation can account for any prefix.
#[derive(Clone)]
pub(crate) struct Queue {
    entries: [Option<Entry>; 4],
    len: usize,
    ticks: u32,
    pulses: u64,
    signed: i64,
    startup_ticks: u32,
    pub direction: Direction,
    pub closed: bool,
}

impl Queue {
    pub fn prepare(&self, config: Config, segments: &[Segment]) -> Result<(Self, [u32; 4]), Error> {
        if segments.len() > 4 {
            return Err(Error::QueueFull);
        }
        let mut words = [0; 4];
        let mut next = self.clone();
        for (word, &segment) in words.iter_mut().zip(segments) {
            let entry = config.encode(segment, next.direction)?;
            next.push(entry, config.max_committed_ticks)?;
            *word = entry.word;
        }
        Ok((next, words))
    }

    pub fn new(direction: Direction) -> Self {
        Self {
            entries: [None; 4],
            len: 0,
            ticks: 0,
            pulses: 0,
            signed: 0,
            startup_ticks: 17,
            direction,
            closed: false,
        }
    }

    pub fn push(&mut self, entry: Entry, budget: u32) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if self.len == 4 {
            return Err(Error::QueueFull);
        }
        let ticks = u64::from(self.ticks) + u64::from(entry.ticks);
        if ticks + u64::from(self.startup_ticks) + 1 > u64::from(budget) {
            return Err(Error::Horizon);
        }
        self.entries[self.len] = Some(entry);
        self.len += 1;
        self.ticks = ticks as u32;
        self.direction = entry.direction;
        self.closed = entry.end;
        Ok(())
    }

    pub fn acknowledge(&mut self) {
        let entry = self.entries[0].expect("PIO completion without committed command");
        assert!(!entry.end, "End never emits a completion token");
        self.pulses = self.pulses.wrapping_add(u64::from(entry.pulses));
        self.signed = self
            .signed
            .wrapping_add(entry.direction.signed(entry.pulses));
        self.ticks -= entry.ticks;
        self.startup_ticks = 0;
        self.entries.rotate_left(1);
        self.len -= 1;
        self.entries[self.len] = None;
    }

    pub fn progress(&self) -> Progress {
        Progress {
            completed_pulses: self.pulses,
            completed_signed_pulses: self.signed,
            outstanding_commands: self.len,
            committed_ticks: if self.len == 0 {
                0
            } else {
                self.ticks + self.startup_ticks + 1
            },
        }
    }

    // PC is relative to the program origin, after disabling the state machine.
    // OUT Y (4) initializes the current count; PUSH (20) retires it. Drain RX
    // before calling: at PC 21 the ledger already refers to the NEXT command.
    pub fn stopped(&self, pc: u8, remaining: u32, reason: StopReason) -> StopReport {
        let partial = self.entries[0].filter(|entry| !entry.end && (5..=20).contains(&pc));
        let pulses = partial.map_or(0, |entry| {
            // Hardware Y decreases on the SAME instruction as the rising edge.
            assert!(remaining <= u32::from(entry.pulses));
            entry.pulses - remaining as u16
        });
        StopReport {
            reason,
            emitted_pulses: self.pulses.wrapping_add(u64::from(pulses)),
            emitted_signed_pulses: self
                .signed
                .wrapping_add(partial.map_or(0, |entry| entry.direction.signed(pulses))),
            pulse_may_be_truncated: (12..=14).contains(&pc),
        }
    }
}

#[cfg(test)]
mod tests;
