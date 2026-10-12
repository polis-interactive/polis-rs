#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

#[cfg(feature = "rs485")]
pub mod rs485;
#[cfg(feature = "udp")]
pub mod udp;

use core::fmt;
use polis_discovery_protocol::MAX_PAYLOAD_LEN;
#[cfg(any(feature = "udp", feature = "rs485"))]
use polis_discovery_protocol::Message;
#[cfg(any(feature = "udp", feature = "rs485"))]
use polis_discovery_protocol::{Discovery, PROTOCOL_ID, SCHEMA_VERSION};
#[cfg(any(feature = "udp", feature = "rs485"))]
use polis_framing::Frame;
#[cfg(any(feature = "udp", feature = "rs485"))]
use polis_framing::Header;

/// Maximum encoded discovery frame, including both delimiters.
pub const WIRE_CAPACITY: usize = polis_framing::wire_capacity(MAX_PAYLOAD_LEN);

/// RS-485 events are delivered before the decoder reuses its buffer.
#[cfg(feature = "rs485")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamEvent<'a> {
    Discovery(Message),
    /// Another protocol's complete frame. Copy/decode before returning.
    Other(Frame<'a>),
}

#[derive(Debug)]
pub enum Error<E> {
    Io(E),
    Framing(polis_framing::Error),
    Protocol(polis_discovery_protocol::Error),
    WriteZero,
    TimedOut,
    Closed,
    DeadlineOverflow,
}

impl<E: fmt::Debug> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "discovery I/O: {e:?}"),
            Self::Framing(e) => write!(f, "discovery framing: {e}"),
            Self::Protocol(e) => write!(f, "discovery protocol: {e}"),
            Self::WriteZero => f.write_str("discovery write made no progress"),
            Self::TimedOut => f.write_str("discovery operation timed out"),
            Self::Closed => f.write_str("discovery stream closed"),
            Self::DeadlineOverflow => f.write_str("discovery deadline exceeds clock range"),
        }
    }
}
impl<E: fmt::Debug> core::error::Error for Error<E> {}

#[cfg(any(feature = "udp", feature = "rs485"))]
fn encode<E>(discovery: &Discovery, message: &Message, wire: &mut [u8]) -> Result<usize, Error<E>> {
    let mut payload = [0; MAX_PAYLOAD_LEN as usize];
    let payload = discovery
        .encode_payload(message, &mut payload)
        .map_err(Error::Protocol)?;
    let bytes = polis_framing::encode(
        Header {
            protocol_id: PROTOCOL_ID,
            schema_version: SCHEMA_VERSION,
            message_type: message.message_type(),
        },
        payload,
        wire,
    )
    .map_err(Error::Framing)?;
    Ok(bytes.len())
}

#[cfg(any(feature = "udp", feature = "rs485"))]
fn decode<E>(discovery: &Discovery, frame: Frame<'_>) -> Result<Message, Error<E>> {
    discovery
        .decode_payload(
            frame.header.protocol_id,
            frame.header.schema_version,
            frame.header.message_type,
            frame.payload,
        )
        .map_err(Error::Protocol)
}
