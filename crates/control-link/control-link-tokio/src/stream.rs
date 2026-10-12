//! Framed, full-duplex point-to-point I/O. No discovery or background tasks.
use crate::Error;
use polis_control_link_protocol::{Buffers, ControlLink, Message};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub type Pair<'a, R, W> = (Sender<'a, W>, Receiver<'a, R>);

/// Keep these halves for the entire connection, including identity/session setup.
pub fn split<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
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
    link: ControlLink,
    writer: W,
    payload: &'a mut [u8],
    wire: &'a mut [u8],
    ready: bool,
}
impl<'a, W: AsyncWrite + Unpin> Sender<'a, W> {
    pub fn new(link: ControlLink, writer: W, payload: &'a mut [u8], wire: &'a mut [u8]) -> Self {
        Self {
            link,
            writer,
            payload,
            wire,
            ready: true,
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
    pub async fn send(&mut self, message: Message<'_>) -> Result<(), Error<std::io::Error>> {
        if !self.ready {
            return Err(Error::Interrupted);
        }
        let wire = self
            .link
            .encode(message, self.payload, self.wire)
            .map_err(Error::Protocol)?;
        self.ready = false;
        let mut remaining = wire;
        while !remaining.is_empty() {
            let n = self.writer.write(remaining).await.map_err(Error::Io)?;
            if n == 0 {
                return Err(Error::WriteZero);
            }
            remaining = &remaining[n..];
        }
        self.writer.flush().await.map_err(Error::Io)?;
        self.ready = true;
        Ok(())
    }
}

/// Persistent decoder and unread chunk. One receive returns one message without
/// discarding later frames from the same read. Never rebuild it after a handshake.
pub struct Receiver<'a, R> {
    link: ControlLink,
    reader: R,
    decoder: polis_framing::Decoder<'a>,
    chunk: [u8; 256],
    cursor: usize,
    len: usize,
    closed: bool,
}
impl<'a, R: AsyncRead + Unpin> Receiver<'a, R> {
    pub fn new(link: ControlLink, reader: R, storage: &'a mut [u8]) -> Self {
        Self {
            link,
            reader,
            decoder: polis_framing::Decoder::new(storage),
            chunk: [0; 256],
            cursor: 0,
            len: 0,
            closed: false,
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
    /// Framing(Truncated) once, then Closed. Tokio reads are cancellation-safe;
    /// all consumed bytes stay here.
    pub async fn receive<T>(
        &mut self,
        decode: impl FnOnce(Message<'_>) -> T,
    ) -> Result<T, Error<std::io::Error>> {
        if self.closed {
            return Err(Error::Closed);
        }
        loop {
            while self.cursor < self.len {
                let byte = self.chunk[self.cursor];
                self.cursor += 1;
                if let Some(frame) = self.decoder.push(byte) {
                    let message = self
                        .link
                        .decode(frame.map_err(Error::Framing)?)
                        .map_err(Error::Protocol)?;
                    return Ok(decode(message));
                }
            }
            self.len = match self.reader.read(&mut self.chunk).await {
                Ok(n) => n,
                Err(e) => {
                    self.closed = true;
                    return Err(Error::Io(e));
                }
            };
            self.cursor = 0;
            if self.len == 0 {
                self.closed = true;
                self.decoder.finish().map_err(Error::Framing)?;
                return Err(Error::Closed);
            }
        }
    }
}
