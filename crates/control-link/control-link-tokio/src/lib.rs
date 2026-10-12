#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]
pub use polis_control_link_protocol as protocol;
pub use protocol::Buffers;
#[cfg(feature = "serial")]
pub mod serial;
pub mod stream;
#[cfg(feature = "tcp")]
pub mod tcp;

/// A transport or codec failure. Interrupted sends permanently retire the sender.
#[derive(Debug)]
pub enum Error<E> {
    Io(E),
    Framing(polis_framing::Error),
    Protocol(polis_control_link_protocol::Error),
    Closed,
    WriteZero,
    Interrupted,
}
impl<E: core::fmt::Display> core::fmt::Display for Error<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O: {e}"),
            Self::Framing(e) => write!(f, "framing: {e}"),
            Self::Protocol(e) => write!(f, "control protocol: {e}"),
            Self::Closed => f.write_str("connection closed"),
            Self::WriteZero => f.write_str("transport wrote zero bytes"),
            Self::Interrupted => {
                f.write_str("previous send was interrupted; replace the connection")
            }
        }
    }
}
impl<E: core::error::Error + 'static> core::error::Error for Error<E> {}
