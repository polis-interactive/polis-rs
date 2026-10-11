//! Private framed I/O for the RS-485 scheduler.
use crate::{Error, StreamEvent, WIRE_CAPACITY, decode, encode};
use core::convert::Infallible;
use discovery_protocol::{Discovery, Message};
use embedded_io_async::{Read, Write};

/// Send one whole discovery frame and flush. Serialize all writers on this link.
/// Cancellation/error can leave a partial frame; the next frame's leading zero
/// resynchronizes the peer. This does not make generic I/O cancellation-safe.
pub(super) async fn send<W: Write + ?Sized>(
    writer: &mut W,
    discovery: &Discovery,
    message: &Message,
) -> Result<(), Error<W::Error>> {
    let mut wire = [0; WIRE_CAPACITY];
    let len = encode(discovery, message, &mut wire)?;
    let mut remaining = &wire[..len];
    while !remaining.is_empty() {
        let n = writer.write(remaining).await.map_err(Error::Io)?;
        if n == 0 {
            return Err(Error::WriteZero);
        }
        remaining = &remaining[n..];
    }
    writer.flush().await.map_err(Error::Io)
}

/// Retains partial frames across reads; caller supplies capacity for all protocols.
/// Use at least WIRE_CAPACITY for discovery-only streams.
pub(super) struct Receiver<'a> {
    discovery: Discovery,
    decoder: framing::Decoder<'a>,
}

impl<'a> Receiver<'a> {
    pub(super) fn new(discovery: Discovery, storage: &'a mut [u8]) -> Self {
        Self {
            discovery,
            decoder: framing::Decoder::new(storage),
        }
    }

    /// Call when changing peers or reconnecting. Never carry a partial frame across links.
    pub(super) fn reset(&mut self) {
        self.decoder.reset();
    }

    /// Process every frame in a chunk, preserving other protocols for the caller.
    /// Callbacks cannot retain borrowed frames: decode/copy before returning.
    pub(super) fn feed(
        &mut self,
        bytes: &[u8],
        mut on_event: impl FnMut(Result<StreamEvent<'_>, Error<Infallible>>),
    ) {
        let discovery = self.discovery;
        self.decoder.feed(bytes, |event| {
            on_event(match event {
                Err(error) => Err(Error::Framing(error)),
                Ok(frame) if frame.header.protocol_id == discovery_protocol::PROTOCOL_ID => {
                    decode(&discovery, frame).map(StreamEvent::Discovery)
                }
                Ok(frame) => Ok(StreamEvent::Other(frame)),
            });
        });
    }

    /// End this stream, report any incomplete frame, and reset for a fresh link.
    pub(super) fn finish(&mut self) -> Result<(), Error<Infallible>> {
        self.decoder.finish().map_err(Error::Framing)
    }

    /// Read one chunk and synchronously dispatch every decoded frame or codec error.
    /// Returns zero at EOF (including any final truncation callback); stop the read loop then.
    /// I/O errors reset the decoder. Wrong-application frames are explicit protocol errors.
    /// Cancellation inherits the Read implementation: use a cancel-safe reader in select/timeout.
    /// DMA readers may consume bytes on cancellation; reset the decoder if that happens.
    pub(super) async fn read<R: Read + ?Sized>(
        &mut self,
        reader: &mut R,
        mut on_event: impl FnMut(Result<StreamEvent<'_>, Error<Infallible>>),
    ) -> Result<usize, R::Error> {
        let mut bytes = [0; 256];
        let len = match reader.read(&mut bytes).await {
            Ok(len) => len,
            Err(error) => {
                self.reset();
                return Err(error);
            }
        };
        if len == 0 {
            if let Err(error) = self.finish() {
                on_event(Err(error));
            }
        } else {
            self.feed(&bytes[..len], on_event);
        }
        Ok(len)
    }

    /// Decode each frame once, dispatch it, then await the transport's message
    /// handler, without buffering a separate reply queue.
    pub(super) async fn dispatch<E>(
        &mut self,
        bytes: &[u8],
        mut on_message: impl AsyncFnMut(Message) -> Result<(), E>,
        mut on_event: impl FnMut(Result<StreamEvent<'_>, Error<Infallible>>),
    ) -> Result<(), E> {
        for &byte in bytes {
            let Some(frame) = self.decoder.push(byte) else {
                continue;
            };
            let event = match frame {
                Err(error) => Err(Error::Framing(error)),
                Ok(frame) if frame.header.protocol_id == discovery_protocol::PROTOCOL_ID => {
                    decode(&self.discovery, frame).map(StreamEvent::Discovery)
                }
                Ok(frame) => Ok(StreamEvent::Other(frame)),
            };
            let message = match &event {
                Ok(StreamEvent::Discovery(message)) => Some(*message),
                _ => None,
            };
            on_event(event);
            if let Some(message) = message
                && let Err(error) = on_message(message).await
            {
                self.reset();
                return Err(error);
            }
        }
        Ok(())
    }
}
