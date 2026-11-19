//! Framed, full-duplex point-to-point I/O. No discovery or background tasks.
use crate::Error;
use embedded_io_async::{Read, Write};
use polis_control_link_protocol::{Buffers, ControlLink, Message};

pub type Pair<'a, R, W> = (Sender<'a, W>, Receiver<'a, R>);

/// Keep these halves for the entire connection, including identity/session setup.
pub fn split<R: Read, W: Write>(
    link: ControlLink,
    reader: R,
    writer: W,
    buffers: Buffers<'_>,
) -> Pair<'_, R, W> {
    (
        Sender::new(link, writer, buffers.payload, buffers.transmit),
        Receiver::new(link, reader, buffers.receive),
    )
}

/// One serialized writer. Dropping a pending send or any I/O failure retires it;
/// a new send returns Interrupted. Replace the connection with a fresh session.
pub struct Sender<'a, W> {
    writer: W,
    state: crate::io::Transmit<'a>,
}
impl<'a, W: Write> Sender<'a, W> {
    pub fn new(link: ControlLink, writer: W, payload: &'a mut [u8], wire: &'a mut [u8]) -> Self {
        Self {
            writer,
            state: crate::io::Transmit::new(link, payload, wire),
        }
    }

    /// Recover the backend when retiring this connection. An interrupted send
    /// must not be resumed or replayed; establish a fresh session before reuse.
    pub fn into_inner(self) -> W {
        self.writer
    }

    /// Encode, write every byte, and flush. A completed write is not evidence that
    /// the peer executed a request: match its Response using protocol::Requests.
    /// Codec errors happen before I/O and leave this sender usable.
    pub async fn send(&mut self, message: Message<'_>) -> Result<(), Error<W::Error>> {
        self.state.send(&mut self.writer, message).await
    }
}

/// Persistent decoder and unread chunk. One receive returns one message without
/// discarding later frames from the same read. Never rebuild it after a handshake.
pub struct Receiver<'a, R> {
    reader: R,
    state: crate::io::Receive<'a>,
}
impl<'a, R: Read> Receiver<'a, R> {
    pub fn new(link: ControlLink, reader: R, storage: &'a mut [u8]) -> Self {
        Self {
            reader,
            state: crate::io::Receive::new(link, storage),
        }
    }

    /// Recover the backend when retiring this connection, discarding buffered data.
    /// Do not use this to hand off from a handshake to normal traffic.
    pub fn into_inner(self) -> R {
        self.reader
    }

    /// Receive one message and decode/copy its borrowed body inside the callback.
    /// Return an owned application value to handle asynchronously outside this call.
    /// Frame/protocol errors are reported individually; the next call can continue.
    /// EOF or I/O failure closes this receiver. EOF in a partial frame reports
    /// Framing(Truncated) once, then Closed. Cancellation retains decoded state,
    /// but cancelling a pending read also requires a cancellation-safe backend.
    pub async fn receive<T>(
        &mut self,
        decode: impl FnOnce(Message<'_>) -> T,
    ) -> Result<T, Error<R::Error>> {
        self.state.receive(&mut self.reader, decode).await
    }
}
