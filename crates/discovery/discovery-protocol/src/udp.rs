//! Shared discovery rendezvous defaults. Consumers may override the endpoint.
//! These values are deployment defaults, not registered service assignments.

/// Shared Polis discovery group: 239, ASCII P, ASCII D, 1.
pub const DISCOVERY_MULTICAST_IPV4: [u8; 4] = [239, 80, 68, 1];
pub const DISCOVERY_PORT: u16 = 43_000;
