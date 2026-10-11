//! Scheduled half-duplex RS-485 discovery, with one controller owning the bus.
//!
//! Supply direction-controlled I/O. A successful send/flush must include the
//! final stop bit and bus release. On cancelled/failed sends, recover the driver
//! before further I/O. Embassy's `rs485` driver supports this contract; a raw UART
//! or a host serial port alone does not. Host hardware/driver must supply RS-485
//! direction control and satisfy the same completion/cancellation guarantees.
//!
//! Responses require a measured upper bound on RX timestamp error and total TX/bus release
//! time. Include timer resolution and scheduling jitter. Unbounded OS/USB latency
//! cannot support a guaranteed slot schedule. No automatic retries or addressing.
use core::num::NonZeroU64;

use discovery_protocol::rs485::Collection;
pub use discovery_protocol::rs485::{DiscoveryRound, Participant, ScanSummary, Timing};
use discovery_protocol::{Announcement, Discovery, Message};
use embassy_time::{Duration, Instant, Timer, with_deadline, with_timeout};
use embedded_io_async::{Read, Write};

mod io;
use crate::{Error, StreamEvent, WIRE_CAPACITY};
use io::Receiver;

/// Handle one decoded message for a bus participant. `observed_at` is captured
/// when the complete round is decoded; Timing bounds the error relative to its final stop
/// bit. ACKs update suppression state without sending. Returns true after a reply,
/// false for ignored/acknowledged/duplicate/late rounds or insufficient slot space.
///
/// The opportunity is consumed before awaiting, so cancellation does not replay
/// the same round. Cancellation during transmission may require driver recovery.
/// Continue feeding received ACKs to this function after a successful response.
pub async fn respond<S: Write + ?Sized>(
    bus: &mut S,
    discovery: &Discovery,
    participant: &mut Participant,
    message: &Message,
    observed_at: Instant,
    timing: Timing,
) -> Result<bool, Error<S::Error>> {
    let Some(window) = participant.receive(message, timing) else {
        return Ok(false);
    };
    let opens = observed_at
        .checked_add(Duration::from_micros(window.opens_us))
        .ok_or(Error::DeadlineOverflow)?;
    let closes = observed_at
        .checked_add(Duration::from_micros(window.closes_us))
        .ok_or(Error::DeadlineOverflow)?;
    Timer::at(opens).await;
    let finish = Instant::now()
        .checked_add(Duration::from_micros(u64::from(
            timing.transmit_budget_us.get(),
        )))
        .ok_or(Error::DeadlineOverflow)?;
    if finish > closes {
        return Ok(false);
    }
    with_deadline(
        closes,
        io::send(bus, discovery, &Message::Announcement(window.announcement)),
    )
    .await
    .map_err(|_| Error::TimedOut)??;
    Ok(true)
}

/// Send one round, collect up to N distinct matching identities, then ACK them
/// only after the complete response window. Corrupt/colliding/unrelated frames
/// are ignored. The callback runs once per retained identity and must be quick.
/// This function exclusively owns reception during discovery; business traffic
/// must wait until it returns. Use the returned overflow flag to schedule retries.
///
/// `send_timeout` bounds each round/ACK send. Collection starts after TX returns:
/// the required physical completion contract makes the ACK deadline conservative.
/// On cancellation/error after sending the round, the caller must keep the bus
/// quiet through its full response window before starting another transmission.
pub async fn discover<const N: usize, S: Read + Write + ?Sized>(
    bus: &mut S,
    discovery: &Discovery,
    round: DiscoveryRound,
    send_timeout: Duration,
    mut on_device: impl FnMut(Announcement),
) -> Result<ScanSummary, Error<S::Error>> {
    let id = NonZeroU64::new(round.discovery_id)
        .ok_or(Error::Protocol(discovery_protocol::Error::ZeroDiscoveryId))?;
    let mut collection = Collection::<N>::new(id);
    with_timeout(
        send_timeout,
        io::send(bus, discovery, &Message::Rs485DiscoveryRound(round)),
    )
    .await
    .map_err(|_| Error::TimedOut)??;
    let end = Instant::now()
        .checked_add(Duration::from_micros(round.response_window_end_us()))
        .ok_or(Error::DeadlineOverflow)?;
    let mut storage = [0; WIRE_CAPACITY];
    let mut receiver = Receiver::new(*discovery, &mut storage);
    loop {
        if Instant::now() >= end {
            break;
        }
        let received = with_deadline(
            end,
            receiver.read(bus, |event| {
                if Instant::now() < end
                    && let Ok(StreamEvent::Discovery(Message::Announcement(value))) = event
                    && collection.record(value)
                {
                    on_device(value);
                }
            }),
        )
        .await;
        match received {
            Err(_) => break,
            Ok(Err(error)) => return Err(Error::Io(error)),
            Ok(Ok(0)) => return Err(Error::Closed),
            Ok(Ok(_)) => {}
        }
    }
    for ack in collection.acknowledgements() {
        with_timeout(
            send_timeout,
            io::send(bus, discovery, &Message::Rs485DiscoveryAck(ack)),
        )
        .await
        .map_err(|_| Error::TimedOut)??;
    }
    Ok(collection.summary())
}

/// Run the participant's receive/response loop on an exclusive half-duplex bus.
/// Discovery and other protocols also reach the synchronous application callback.
/// The provided storage must fit the largest protocol on this bus. Discard stale
/// RX data before starting so Timing's timestamp-error bound can hold from the outset.
/// A read's completion timestamp anchors every frame completed by that read;
/// delayed dispatch cannot move a reply's deadline into a later slot.
///
/// Cancel only when ending this receive session; a pending response can leave
/// unread bytes from the current chunk. Recover a cancelled send before reuse.
pub async fn serve<S: Read + Write + ?Sized>(
    bus: &mut S,
    discovery: &Discovery,
    participant: &mut Participant,
    timing: Timing,
    storage: &mut [u8],
    mut on_event: impl FnMut(Result<StreamEvent<'_>, Error<core::convert::Infallible>>),
) -> Result<(), Error<S::Error>> {
    let mut receiver = Receiver::new(*discovery, storage);
    let mut bytes = [0; 256];
    loop {
        let len = bus.read(&mut bytes).await.map_err(Error::Io)?;
        let observed_at = Instant::now();
        if len == 0 {
            return Err(Error::Closed);
        }
        receiver
            .dispatch(
                &bytes[..len],
                async |message| {
                    respond(bus, discovery, participant, &message, observed_at, timing).await?;
                    Ok(())
                },
                &mut on_event,
            )
            .await?;
    }
}
