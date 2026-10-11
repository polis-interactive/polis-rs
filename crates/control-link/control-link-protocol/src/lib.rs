#![no_std]
#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod tracking;

pub use tracking::{Incoming, RequestGate, Requests, StateError};

use core::{fmt, num::NonZeroU32, num::NonZeroU64};
use framing::{Frame, Header};

/// Shared control protocol. Discovery has its own independent protocol ID.
pub const PROTOCOL_ID: u16 = 0x5043;
pub const SCHEMA_VERSION: u16 = 1;
/// Application ID (16), session (8), and request ID (4), before opaque body bytes.
pub const PREFIX_LEN: usize = 28;
pub const MAX_BODY_LEN: usize = u16::MAX as usize - PREFIX_LEN;

/// Correlation key. Both fields are nonzero; IDs must increase within a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub session: NonZeroU64,
    pub id: NonZeroU32,
}

/// Explicit message IDs in the framing header, independent of Rust enum layout.
pub mod message_type {
    pub const REQUEST: u16 = 1;
    pub const ACCEPTED: u16 = 2;
    pub const RESPONSE: u16 = 3;
    pub const EVENT: u16 = 4;
}

/// Application-owned bodies can contain any encoding, including Postcard.
/// A response is terminal (success or application error); Accepted is optional.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message<'a> {
    Request { key: Key, body: &'a [u8] },
    Accepted { key: Key },
    Response { key: Key, body: &'a [u8] },
    Event { session: NonZeroU64, body: &'a [u8] },
}

impl<'a> Message<'a> {
    pub const fn message_type(self) -> u16 {
        match self {
            Self::Request { .. } => message_type::REQUEST,
            Self::Accepted { .. } => message_type::ACCEPTED,
            Self::Response { .. } => message_type::RESPONSE,
            Self::Event { .. } => message_type::EVENT,
        }
    }

    pub const fn session(self) -> NonZeroU64 {
        match self {
            Self::Request { key, .. } | Self::Accepted { key } | Self::Response { key, .. } => {
                key.session
            }
            Self::Event { session, .. } => session,
        }
    }

    pub const fn body(self) -> &'a [u8] {
        match self {
            Self::Request { body, .. } | Self::Response { body, .. } | Self::Event { body, .. } => {
                body
            }
            Self::Accepted { .. } => &[],
        }
    }

    const fn request_id(self) -> u32 {
        match self {
            Self::Request { key, .. } | Self::Accepted { key } | Self::Response { key, .. } => {
                key.id.get()
            }
            Self::Event { .. } => 0,
        }
    }
}

/// Codec for one application, using the same caller-assigned 16 bytes as discovery.
/// Owns no transport, executor, session, or business handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlLink {
    application: [u8; 16],
}

impl ControlLink {
    pub const fn new(application: [u8; 16]) -> Self {
        Self { application }
    }

    pub const fn application(&self) -> [u8; 16] {
        self.application
    }

    /// Encode a complete shared frame. Scratch holds PREFIX_LEN + body.len();
    /// wire needs framing::wire_capacity() for that total payload length.
    /// Never transmit either buffer after an error.
    pub fn encode<'a>(
        &self,
        message: Message<'_>,
        scratch: &mut [u8],
        wire: &'a mut [u8],
    ) -> Result<&'a [u8], Error> {
        let body = message.body();
        if body.len() > MAX_BODY_LEN {
            return Err(Error::BodyTooLarge);
        }
        let len = PREFIX_LEN + body.len();
        let payload = scratch.get_mut(..len).ok_or(Error::OutputTooSmall)?;
        payload[..16].copy_from_slice(&self.application);
        payload[16..24].copy_from_slice(&message.session().get().to_le_bytes());
        payload[24..28].copy_from_slice(&message.request_id().to_le_bytes());
        payload[PREFIX_LEN..].copy_from_slice(body);
        framing::encode(
            Header {
                protocol_id: PROTOCOL_ID,
                schema_version: SCHEMA_VERSION,
                message_type: message.message_type(),
            },
            payload,
            wire,
        )
        .map_err(Error::Framing)
    }

    /// Decode an already validated frame. The returned body borrows its payload.
    /// Route discovery frames to discovery-protocol instead of this method.
    pub fn decode<'a>(&self, frame: Frame<'a>) -> Result<Message<'a>, Error> {
        if frame.header.protocol_id != PROTOCOL_ID {
            return Err(Error::UnknownProtocol(frame.header.protocol_id));
        }
        if frame.header.schema_version != SCHEMA_VERSION {
            return Err(Error::UnsupportedSchema(frame.header.schema_version));
        }
        let application = frame.payload.get(..16).ok_or(Error::Truncated)?;
        if application != self.application {
            return Err(Error::WrongApplication);
        }
        let prefix = frame.payload.get(..PREFIX_LEN).ok_or(Error::Truncated)?;
        let session = NonZeroU64::new(u64::from_le_bytes(prefix[16..24].try_into().unwrap()))
            .ok_or(Error::ZeroSession)?;
        let id = u32::from_le_bytes(prefix[24..28].try_into().unwrap());
        let body = &frame.payload[PREFIX_LEN..];
        if body.len() > MAX_BODY_LEN {
            return Err(Error::BodyTooLarge);
        }
        if frame.header.message_type == message_type::EVENT {
            if id != 0 {
                return Err(Error::EventHasRequestId);
            }
            return Ok(Message::Event { session, body });
        }
        let key = Key {
            session,
            id: NonZeroU32::new(id).ok_or(Error::ZeroRequestId)?,
        };
        match frame.header.message_type {
            message_type::REQUEST => Ok(Message::Request { key, body }),
            message_type::ACCEPTED if body.is_empty() => Ok(Message::Accepted { key }),
            message_type::ACCEPTED => Err(Error::AcceptedHasBody),
            message_type::RESPONSE => Ok(Message::Response { key, body }),
            other => Err(Error::UnknownMessageType(other)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Framing(framing::Error),
    OutputTooSmall,
    BodyTooLarge,
    UnknownProtocol(u16),
    UnsupportedSchema(u16),
    UnknownMessageType(u16),
    WrongApplication,
    Truncated,
    ZeroSession,
    ZeroRequestId,
    EventHasRequestId,
    AcceptedHasBody,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(error) => write!(f, "framing: {error}"),
            Self::OutputTooSmall => f.write_str("control payload buffer is too small"),
            Self::BodyTooLarge => f.write_str("control body exceeds the framing payload limit"),
            Self::UnknownProtocol(id) => write!(f, "unknown protocol {id}"),
            Self::UnsupportedSchema(version) => write!(f, "unsupported control schema {version}"),
            Self::UnknownMessageType(id) => write!(f, "unknown control message type {id}"),
            Self::WrongApplication => f.write_str("control message belongs to another application"),
            Self::Truncated => f.write_str("incomplete control prefix"),
            Self::ZeroSession => f.write_str("control session must be nonzero"),
            Self::ZeroRequestId => f.write_str("control request ID must be nonzero"),
            Self::EventHasRequestId => f.write_str("events must have request ID zero"),
            Self::AcceptedHasBody => f.write_str("Accepted must have an empty body"),
        }
    }
}

impl core::error::Error for Error {}

/// Caller-owned storage for one full-duplex connection. No allocation is required.
/// For a maximum body of B bytes, payload needs PREFIX_LEN + B bytes; receive and
/// transmit each need framing::wire_capacity((PREFIX_LEN + B) as u16).
/// Keep B <= MAX_BODY_LEN. Buffers remain with the connection through its handshake.
pub struct Buffers<'a> {
    pub receive: &'a mut [u8],
    pub payload: &'a mut [u8],
    pub transmit: &'a mut [u8],
}
