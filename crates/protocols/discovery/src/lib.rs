#![no_std]
#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod messages;
pub mod rs485;
pub mod udp;

pub use messages::{Announcement, Discover};

use core::fmt;
use serde::{Deserialize, Serialize};

use rs485::{DiscoveryAck, DiscoveryRound};

/// Shared Polis discovery protocol identifier, independent of application identity.
pub const PROTOCOL_ID: u16 = 0x5044;
pub const SCHEMA_VERSION: u16 = 1;
/// Fixed application identifier prefix in every discovery payload.
pub const APPLICATION_ID_LEN: usize = 16;
/// Application prefix plus the largest schema-1 body (an RS-485 round).
pub const MAX_PAYLOAD_LEN: u16 = 44;

/// Stable identity across links and restarts. All eight-byte values are ordinary IDs.
/// Provisioning must ensure uniqueness; this type does not generate IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub [u8; 8]);

/// Explicit header assignments; these are not Serde enum discriminants.
pub mod message_type {
    pub const DISCOVER: u16 = 0x0001;
    pub const ANNOUNCEMENT: u16 = 0x0002;
    pub const RS485_DISCOVERY_ROUND: u16 = 0x0101;
    pub const RS485_DISCOVERY_ACK: u16 = 0x0102;
}

/// In-memory dispatch only. Encode the variant's payload using [`Discovery::encode_payload`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    Discover(Discover),
    Announcement(Announcement),
    Rs485DiscoveryRound(DiscoveryRound),
    Rs485DiscoveryAck(DiscoveryAck),
}

impl Message {
    pub const fn message_type(&self) -> u16 {
        match self {
            Self::Discover(_) => message_type::DISCOVER,
            Self::Announcement(_) => message_type::ANNOUNCEMENT,
            Self::Rs485DiscoveryRound(_) => message_type::RS485_DISCOVERY_ROUND,
            Self::Rs485DiscoveryAck(_) => message_type::RS485_DISCOVERY_ACK,
        }
    }

    /// Encode only the Postcard payload into caller-owned memory.
    /// The transport places the IDs in a framing header, then frames these bytes.
    fn encode_body<'a>(&self, output: &'a mut [u8]) -> Result<&'a [u8], Error> {
        self.validate()?;
        let bytes = match self {
            Self::Discover(value) => postcard::to_slice(value, output),
            Self::Announcement(value) => postcard::to_slice(value, output),
            Self::Rs485DiscoveryRound(value) => postcard::to_slice(value, output),
            Self::Rs485DiscoveryAck(value) => postcard::to_slice(value, output),
        }
        .map_err(Error::Payload)?;
        Ok(bytes)
    }

    fn decode_body(message_type: u16, payload: &[u8]) -> Result<Self, Error> {
        let message = match message_type {
            message_type::DISCOVER => Self::Discover(decode(payload)?),
            message_type::ANNOUNCEMENT => Self::Announcement(decode(payload)?),
            message_type::RS485_DISCOVERY_ROUND => Self::Rs485DiscoveryRound(decode(payload)?),
            message_type::RS485_DISCOVERY_ACK => Self::Rs485DiscoveryAck(decode(payload)?),
            unknown => return Err(Error::UnknownMessageType(unknown)),
        };
        message.validate()?;
        Ok(message)
    }

    fn validate(&self) -> Result<(), Error> {
        let discovery_id = match self {
            Self::Discover(value) => value.discovery_id,
            // Zero is reserved for unsolicited UDP announcements.
            Self::Announcement(_) => return Ok(()),
            Self::Rs485DiscoveryRound(value) => value.discovery_id,
            Self::Rs485DiscoveryAck(value) => value.discovery_id,
        };
        if discovery_id == 0 {
            return Err(Error::ZeroDiscoveryId);
        }
        Ok(())
    }
}

fn decode<'a, T: Deserialize<'a>>(payload: &'a [u8]) -> Result<T, Error> {
    let (value, rest) = postcard::take_from_bytes(payload).map_err(Error::Payload)?;
    if !rest.is_empty() {
        return Err(Error::TrailingPayload);
    }
    Ok(value)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    UnknownProtocol(u16),
    TruncatedApplicationId,
    WrongApplication(ApplicationId),
    OutputTooSmall,
    UnsupportedSchema(u16),
    UnknownMessageType(u16),
    Payload(postcard::Error),
    TrailingPayload,
    ZeroDiscoveryId,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedApplicationId => f.write_str("missing discovery application identifier"),
            Self::WrongApplication(id) => {
                write!(f, "discovery belongs to application {:02x?}", id.0)
            }
            Self::OutputTooSmall => {
                f.write_str("discovery output is smaller than the application prefix")
            }
            Self::UnknownProtocol(id) => write!(f, "unknown protocol {id}"),
            Self::UnsupportedSchema(version) => write!(f, "unsupported schema {version}"),
            Self::UnknownMessageType(id) => write!(f, "unknown message type {id}"),
            Self::Payload(error) => write!(f, "invalid protocol payload: {error}"),
            Self::TrailingPayload => f.write_str("trailing protocol payload bytes"),
            Self::ZeroDiscoveryId => {
                f.write_str("discovery requests and acknowledgments need a nonzero ID")
            }
        }
    }
}

impl core::error::Error for Error {}

/// Consumer-assigned application namespace. Use the same 16 bytes in all peers.
/// No values are reserved; deployments coordinate assignment. This is not a secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ApplicationId(pub [u8; APPLICATION_ID_LEN]);

/// Shared discovery codec configured for one consumer-defined application.
/// Owns no socket, clock, driver, allocator or list of known products.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Discovery {
    application_id: ApplicationId,
}

impl Discovery {
    pub const fn new(application_id: ApplicationId) -> Self {
        Self { application_id }
    }

    pub const fn application_id(&self) -> ApplicationId {
        self.application_id
    }

    /// Prefix a validated message body with this application's fixed identifier.
    /// On error, output may be partially written and must not be transmitted.
    pub fn encode_payload<'a>(
        &self,
        message: &Message,
        output: &'a mut [u8],
    ) -> Result<&'a [u8], Error> {
        if output.len() < APPLICATION_ID_LEN {
            return Err(Error::OutputTooSmall);
        }
        let (application, body) = output.split_at_mut(APPLICATION_ID_LEN);
        let body_len = message.encode_body(body)?.len();
        application.copy_from_slice(&self.application_id.0);
        Ok(&output[..APPLICATION_ID_LEN + body_len])
    }

    /// Validate protocol, schema and application before interpreting the body.
    /// Wrong-application frames may be ignored by the transport. Application
    /// identity is filtering, not authentication or authorization to transmit.
    pub fn decode_payload(
        &self,
        protocol_id: u16,
        schema_version: u16,
        message_type: u16,
        payload: &[u8],
    ) -> Result<Message, Error> {
        if protocol_id != PROTOCOL_ID {
            return Err(Error::UnknownProtocol(protocol_id));
        }
        if schema_version != SCHEMA_VERSION {
            return Err(Error::UnsupportedSchema(schema_version));
        }
        let application = payload
            .get(..APPLICATION_ID_LEN)
            .ok_or(Error::TruncatedApplicationId)?;
        let application = ApplicationId(application.try_into().unwrap());
        if application != self.application_id {
            return Err(Error::WrongApplication(application));
        }
        Message::decode_body(message_type, &payload[APPLICATION_ID_LEN..])
    }
}
