//! RS-485 discovery wire contracts, without a bus, clock, driver, or state machine.

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
