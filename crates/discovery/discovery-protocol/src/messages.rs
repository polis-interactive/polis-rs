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
    /// Echo the request/session ID; zero means unsolicited announcement on UDP.
    pub discovery_id: u64,
    pub device_id: DeviceId,
}

/// Build a reply on UDP. RS-485 rounds need their own
/// scheduled responder and are deliberately ignored here.
pub fn reply_to(message: &crate::Message, device_id: DeviceId) -> Option<Announcement> {
    match message {
        crate::Message::Discover(request) if request.discovery_id != 0 => Some(Announcement {
            discovery_id: request.discovery_id,
            device_id,
        }),
        _ => None,
    }
}

impl Announcement {
    /// Announce without a probe on UDP.
    /// This does not grant permission to transmit on a shared RS-485 bus.
    pub const fn unsolicited(device_id: DeviceId) -> Self {
        Self {
            discovery_id: 0,
            device_id,
        }
    }
}
