//! Shared persistent framing state for split streams and exclusive half-duplex I/O.
use crate::Error;
use embedded_io_async::{Read, Write};
use polis_control_link_protocol::{ControlLink, Message};

pub(crate) struct Transmit<'a> {
    link: ControlLink,
    payload: &'a mut [u8],
    wire: &'a mut [u8],
    ready: bool,
}
impl<'a> Transmit<'a> {
    pub fn new(link: ControlLink, payload: &'a mut [u8], wire: &'a mut [u8]) -> Self {
        Self {
            link,
            payload,
            wire,
            ready: true,
        }
    }

    pub(crate) fn ready(&self) -> bool {
        self.ready
    }

    /// Encode, write every byte, and flush. A completed write is not evidence that
    /// the peer executed a request: match its Response using protocol::Requests.
    /// Codec errors happen before I/O and leave this sender usable.
    pub async fn send<W: Write>(
        &mut self,
        writer: &mut W,
        message: Message<'_>,
    ) -> Result<(), Error<W::Error>> {
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
            let n = writer.write(remaining).await.map_err(Error::Io)?;
            if n == 0 {
                return Err(Error::WriteZero);
            }
            remaining = &remaining[n..];
        }
        writer.flush().await.map_err(Error::Io)?;
        self.ready = true;
        Ok(())
    }
}

pub(crate) struct Receive<'a> {
    link: ControlLink,
    decoder: polis_framing::Decoder<'a>,
    chunk: [u8; 256],
    cursor: usize,
    len: usize,
    closed: bool,
}
impl<'a> Receive<'a> {
    pub fn new(link: ControlLink, storage: &'a mut [u8]) -> Self {
        Self {
            link,
            decoder: polis_framing::Decoder::new(storage),
            chunk: [0; 256],
            cursor: 0,
            len: 0,
            closed: false,
        }
    }

    pub(crate) fn closed(&self) -> bool {
        self.closed
    }

    /// Receive one message and decode/copy its borrowed body inside the callback.
    /// Return an owned application value to handle asynchronously outside this call.
    /// Frame/protocol errors are reported individually; the next call can continue.
    /// EOF or I/O failure closes this receiver. EOF in a partial frame reports
    /// Framing(Truncated) once, then Closed. Cancellation retains decoded state,
    /// but cancelling a pending read also requires a cancellation-safe backend.
    pub async fn receive<R: Read, T>(
        &mut self,
        reader: &mut R,
        decode: impl FnOnce(Message<'_>) -> T,
    ) -> Result<T, Error<R::Error>> {
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
            self.len = match reader.read(&mut self.chunk).await {
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
