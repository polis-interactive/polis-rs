//! RS-485 discovery contracts and portable state, without bus I/O or a clock.

use core::num::{NonZeroU16, NonZeroU32};
use serde::{Deserialize, Serialize};

use crate::DeviceId;

/// The controller grants a bounded response window to undiscovered devices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryRound {
    /// Nonzero session ID. Keep it across retry rounds; change it to rediscover
    /// acknowledged devices. Must not be reused while peers retain that session.
    pub discovery_id: u64,
    /// Change for each retry round so colliding devices select new slots.
    pub seed: u32,
    pub slot_count: NonZeroU16,
    /// Includes the whole announcement, driver release and clock/propagation margin.
    pub slot_duration_us: NonZeroU32,
    /// Delay from the final stop bit of the round frame to the start of slot zero.
    pub turnaround_us: u32,
}

/// Acknowledges an announcement after the complete round response window.
/// Suppresses this device's replies for this session; does not assign a bus address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryAck {
    pub discovery_id: u64,
    pub device_id: DeviceId,
}

impl DiscoveryRound {
    /// Interoperable slot selection: FNV-1a/32 over seed LE bytes, then device ID
    /// bytes in stored order, followed by the MurmurHash3 fmix32 avalanche, then
    /// modulo slot_count. The avalanche mixes high bits into power-of-two slot
    /// counts; using FNV's low bits alone leaves persistent collisions.
    /// This is not a security hash.
    pub fn slot_for(&self, device_id: DeviceId) -> u16 {
        let mut hash = 0x811c_9dc5u32;
        for byte in self.seed.to_le_bytes().into_iter().chain(device_id.0) {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(0x0100_0193);
        }
        hash ^= hash >> 16;
        hash = hash.wrapping_mul(0x85eb_ca6b);
        hash ^= hash >> 13;
        hash = hash.wrapping_mul(0xc2b2_ae35);
        hash ^= hash >> 16;
        (hash % u32::from(self.slot_count.get())) as u16
    }

    /// End of the complete response window, in microseconds after the final
    /// stop bit of the round frame. u64 arithmetic covers all wire values.
    pub fn response_window_end_us(&self) -> u64 {
        u64::from(self.turnaround_us)
            + u64::from(self.slot_count.get()) * u64::from(self.slot_duration_us.get())
    }
}

/// Bounds supplied by the bus integration, in microseconds. They must cover
/// propagation, driver buffering, executor/clock jitter, DE setup, frame transmission
/// and bus release. These are hardware guarantees, not inferred from baud alone.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// Maximum absolute error between the observation timestamp and the round's
    /// final stop bit at the controller (including propagation, early RX notification
    /// or late buffered delivery).
    pub receive_margin_us: u32,
    /// Maximum time from starting the send until the bus is released.
    pub transmit_budget_us: NonZeroU32,
}

/// Safe response interval relative to the caller's round observation timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplyWindow {
    pub opens_us: u64,
    pub closes_us: u64,
    pub announcement: crate::Announcement,
}

/// Device-side discovery state. ACKs suppress replies until a new session ID.
/// Each seed is handled once consecutively, including skipped/cancelled replies;
/// the controller must change the seed for each retry and not reuse old sessions.
pub struct Participant {
    device_id: DeviceId,
    session: u64,
    acknowledged: bool,
    last_seed: Option<u32>,
}

impl Participant {
    pub const fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            session: 0,
            acknowledged: false,
            last_seed: None,
        }
    }

    /// Process a validated discovery message. Never answers an ordinary Discover.
    /// Returns None for ACKs, duplicates, acknowledged sessions or slots too short
    /// for the supplied bounds. No transmission occurs here.
    pub fn receive(&mut self, message: &crate::Message, timing: Timing) -> Option<ReplyWindow> {
        match message {
            crate::Message::Rs485DiscoveryAck(ack)
                if ack.discovery_id != 0
                    && ack.discovery_id == self.session
                    && ack.device_id == self.device_id =>
            {
                self.acknowledged = true;
                None
            }
            crate::Message::Rs485DiscoveryRound(round) if round.discovery_id != 0 => {
                if self.session != round.discovery_id {
                    self.session = round.discovery_id;
                    self.acknowledged = false;
                    self.last_seed = None;
                }
                if self.acknowledged || self.last_seed == Some(round.seed) {
                    return None;
                }
                self.last_seed = Some(round.seed);
                let slot_start_us = u64::from(round.turnaround_us)
                    + u64::from(round.slot_for(self.device_id))
                        * u64::from(round.slot_duration_us.get());
                // Intersect all possible physical slots for timestamps within
                // +/- receive_margin_us of the actual final stop bit.
                let opens_us = slot_start_us + u64::from(timing.receive_margin_us);
                let closes_us = (slot_start_us + u64::from(round.slot_duration_us.get()))
                    .checked_sub(u64::from(timing.receive_margin_us))?;
                if opens_us.checked_add(u64::from(timing.transmit_budget_us.get()))? > closes_us {
                    return None;
                }
                Some(ReplyWindow {
                    opens_us,
                    closes_us,
                    announcement: crate::Announcement {
                        discovery_id: self.session,
                        device_id: self.device_id,
                    },
                })
            }
            _ => None,
        }
    }
}

/// Bounded per-round identities retained until the response window ends, so ACKs
/// cannot collide with later response slots. This is not the application's registry.
pub struct Collection<const N: usize> {
    session: core::num::NonZeroU64,
    devices: [DeviceId; N],
    len: usize,
    overflow: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanSummary {
    pub found: usize,
    /// Some distinct replies could not be retained/acknowledged; retry the same
    /// session with a new seed after the ACK phase to collect remaining devices.
    pub overflow: bool,
}

impl<const N: usize> Collection<N> {
    pub const fn new(session: core::num::NonZeroU64) -> Self {
        Self {
            session,
            devices: [DeviceId([0; 8]); N],
            len: 0,
            overflow: false,
        }
    }

    /// Record a correlated announcement. True exactly once per retained device.
    /// Caller must only feed replies received within this round's response window.
    pub fn record(&mut self, announcement: crate::Announcement) -> bool {
        if announcement.discovery_id != self.session.get()
            || self.devices[..self.len].contains(&announcement.device_id)
        {
            return false;
        }
        if self.len == N {
            self.overflow = true;
            return false;
        }
        self.devices[self.len] = announcement.device_id;
        self.len += 1;
        true
    }

    /// Send these only AFTER the entire response window. Missing ACKs are harmless:
    /// the device replies again on the next seed until it receives its ACK.
    pub fn acknowledgements(&self) -> impl Iterator<Item = DiscoveryAck> + '_ {
        self.devices[..self.len]
            .iter()
            .map(|&device_id| DiscoveryAck {
                discovery_id: self.session.get(),
                device_id,
            })
    }

    pub const fn summary(&self) -> ScanSummary {
        ScanSummary {
            found: self.len,
            overflow: self.overflow,
        }
    }
}
