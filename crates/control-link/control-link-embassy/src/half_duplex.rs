//! Exclusive, point-to-point framed I/O over one half-duplex transport.
//!
//! Use a configured `polis_rs485::Rs485` (after its initial recovery) or another
//! `Read + Write` backend that owns electrical direction and physical TX completion.
//! Electrical cancellation/recovery follows the backend's contract: this adapter
//! cannot abort UART transmission or manipulate DE itself.
//! No mutex, tasks, allocation, arbitration, automatic retries or discovery.
use crate::{Buffers, Error, io, protocol::ControlLink, protocol::Message};
use embedded_io_async::{Read, Write};

/// One transport and persistent framing state for the entire session.
///
/// A single task owns this link. Both methods borrow it exclusively, so a pending
/// receive must complete or be cancelled before sending. The application grants
/// bus turns and supplies deadlines, identity checks, request trackers and queues.
/// Merely obtaining `&mut Link` does not mean the remote peer has released the bus.
///
/// This is a fixed two-peer link, not an addressed multidrop protocol. Either peer
/// may originate messages when its application grants it a transmit window.
pub struct Link<'a, S> {
    transport: S,
    transmit: io::Transmit<'a>,
    receive: io::Receive<'a>,
}

impl<'a, S: Read + Write> Link<'a, S> {
    /// Construct around a ready transport. Keep this object through the handshake
    /// and subsequent traffic; constructing a replacement discards receive state.
    /// Construction performs no I/O or implicit backend recovery.
    pub fn new(transport: S, link: ControlLink, buffers: Buffers<'a>) -> Self {
        Self {
            transport,
            transmit: io::Transmit::new(link, buffers.payload, buffers.transmit),
            receive: io::Receive::new(link, buffers.receive),
        }
    }

    /// Encode one complete frame, write it, and flush before returning.
    ///
    /// The backend must hold direction across the frame and return to receive
    /// after physical TX completion. `Rs485::write` provides this contract even
    /// when its underlying UART writes short chunks. Codec failures precede I/O
    /// and leave the link usable. Any I/O failure or cancellation after encoding
    /// retires BOTH directions; later operations return `Error::Interrupted`.
    ///
    /// A successful send does not confirm execution. Match a Response using
    /// `protocol::Requests`. Apply a deadline to the entire send, including flush.
    pub async fn send(&mut self, message: Message<'_>) -> Result<(), Error<S::Error>> {
        self.ensure_open()?;
        self.transmit.send(&mut self.transport, message).await
    }

    /// Receive one message, retaining partial frames and unread/coalesced bytes.
    /// The synchronous callback may decode/copy a body for subsequent async work.
    ///
    /// Cancellation preserves framing state, but is safe only when the backend's
    /// pending read is cancellation-safe. A receive timeout does not itself retire
    /// this link or grant permission to transmit: the caller must enforce reply
    /// windows and reject late/stale messages. Framing/protocol errors are
    /// recoverable; EOF or an I/O error closes BOTH directions. Partial EOF reports
    /// `Framing(Truncated)` once, then subsequent operations return `Closed`.
    pub async fn receive<T>(
        &mut self,
        decode: impl FnOnce(Message<'_>) -> T,
    ) -> Result<T, Error<S::Error>> {
        self.ensure_open()?;
        self.receive.receive(&mut self.transport, decode).await
    }

    /// Retire the link and recover ownership of its backend, discarding buffered
    /// receive data. This does NOT recover the hardware or purge its RX buffers.
    ///
    /// After interrupted RS-485 TX, call `bus.recover().await` with a deadline to
    /// drain queued TX with DE inactive. Construct a new link, invalidate uncertain
    /// application work and negotiate a fresh session before issuing new commands.
    /// Never replay an interrupted command automatically. Recovery of physical I/O
    /// and recovery of application/session state are separate responsibilities.
    pub fn into_inner(self) -> S {
        self.transport
    }

    fn ensure_open(&self) -> Result<(), Error<S::Error>> {
        if !self.transmit.ready() {
            Err(Error::Interrupted)
        } else if self.receive.closed() {
            Err(Error::Closed)
        } else {
            Ok(())
        }
    }
}
