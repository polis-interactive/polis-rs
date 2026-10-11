//! Common discovery meanings. A transport decides when an announcement may be sent.

use serde::{Deserialize, Serialize};

use crate::DeviceId;

/// Ask devices to announce themselves. On RS-485 use `rs485::DiscoveryRound`
/// instead, because this message alone does not grant a response slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discover {
    /// Nonzero correlation ID selected by the discovery initiator.
    pub discovery_id: u64,
}

/// Identity observed by discovery. Route information belongs to the receiving
/// transport, not the device's identity or this payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Announcement {
    /// Echo the request/session ID; zero means unsolicited UDP announcement.
    pub discovery_id: u64,
    pub device_id: DeviceId,
}
