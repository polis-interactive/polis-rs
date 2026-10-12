//! UDP discovery over a caller-supplied Embassy socket and running network stack.
use core::num::NonZeroU64;

use embassy_net::{
    IpEndpoint,
    udp::{RecvError, SendError, UdpSocket},
};
use embassy_time::{Duration, with_timeout};
use polis_discovery_protocol::{Announcement, Discover, Discovery, Message};

use crate::{Error, WIRE_CAPACITY, decode, encode};

pub fn multicast_endpoint() -> IpEndpoint {
    IpEndpoint::new(
        embassy_net::IpAddress::v4(
            polis_discovery_protocol::udp::DISCOVERY_MULTICAST_IPV4[0],
            polis_discovery_protocol::udp::DISCOVERY_MULTICAST_IPV4[1],
            polis_discovery_protocol::udp::DISCOVERY_MULTICAST_IPV4[2],
            polis_discovery_protocol::udp::DISCOVERY_MULTICAST_IPV4[3],
        ),
        polis_discovery_protocol::udp::DISCOVERY_PORT,
    )
}

pub async fn send(
    socket: &UdpSocket<'_>,
    discovery: &Discovery,
    message: &Message,
    destination: IpEndpoint,
) -> Result<(), Error<SendError>> {
    let mut wire = [0; WIRE_CAPACITY];
    let len = encode(discovery, message, &mut wire)?;
    socket
        .send_to(&wire[..len], destination)
        .await
        .map_err(Error::Io)
}

/// Receive exactly one datagram. Embassy reports truncation as RecvError::Truncated.
/// Returns the sender's endpoint so replies can go to its actual source port.
pub async fn receive(
    socket: &UdpSocket<'_>,
    discovery: &Discovery,
) -> Result<(Message, IpEndpoint), Error<RecvError>> {
    let mut wire = [0; WIRE_CAPACITY];
    let (len, metadata) = socket.recv_from(&mut wire).await.map_err(Error::Io)?;
    let frame =
        polis_framing::decode_datagram(&mut wire[..len], WIRE_CAPACITY).map_err(Error::Framing)?;
    Ok((decode(discovery, frame)?, metadata.endpoint))
}

/// Send one probe and collect correlated replies. The send has its own
/// window-length timeout, followed by a fixed response window. The callback owns
/// deduplication/registration. Invalid, truncated and unrelated packets are ignored.
pub async fn discover(
    socket: &UdpSocket<'_>,
    discovery: &Discovery,
    id: NonZeroU64,
    destination: IpEndpoint,
    window: Duration,
    mut on_device: impl FnMut(Announcement, IpEndpoint),
) -> Result<(), Error<SendError>> {
    let probe = Message::Discover(Discover {
        discovery_id: id.get(),
    });
    with_timeout(window, send(socket, discovery, &probe, destination))
        .await
        .map_err(|_| Error::TimedOut)??;
    let collect = async {
        loop {
            if let Ok((Message::Announcement(value), peer)) = receive(socket, discovery).await
                && value.discovery_id == id.get()
            {
                on_device(value, peer);
            }
        }
    };
    let _ = with_timeout(window, collect).await;
    Ok(())
}

/// Send one unsolicited announcement. Call again at the application's chosen
/// interval, or only at startup; this function owns no background task.
pub async fn announce(
    socket: &UdpSocket<'_>,
    discovery: &Discovery,
    device_id: polis_discovery_protocol::DeviceId,
    destination: IpEndpoint,
) -> Result<(), Error<SendError>> {
    send(
        socket,
        discovery,
        &Message::Announcement(Announcement::unsolicited(device_id)),
        destination,
    )
    .await
}

/// Continuously receive announcements and optionally respond to probes with the
/// supplied identity. This owns the socket's receive loop; do not concurrently run
/// discover/receive/listen on that socket. A single loop supports both roles.
/// Malformed/foreign packets are ignored. Cancel the future to stop; callbacks
/// must return promptly. Application identity is a filter, not authentication.
pub async fn serve(
    socket: &UdpSocket<'_>,
    discovery: &Discovery,
    identity: Option<polis_discovery_protocol::DeviceId>,
    mut on_device: impl FnMut(Announcement, IpEndpoint),
) -> Result<(), Error<SendError>> {
    loop {
        match receive(socket, discovery).await {
            Ok((Message::Announcement(value), peer)) => on_device(value, peer),
            Ok((message, peer)) => {
                if let Some(reply) =
                    identity.and_then(|id| polis_discovery_protocol::reply_to(&message, id))
                {
                    send(socket, discovery, &Message::Announcement(reply), peer).await?;
                }
            }
            _ => {}
        }
    }
}

/// Passively listen for announcements, without sending probes or replies.
pub async fn listen(
    socket: &UdpSocket<'_>,
    discovery: &Discovery,
    on_device: impl FnMut(Announcement, IpEndpoint),
) -> Result<(), Error<SendError>> {
    serve(socket, discovery, None, on_device).await
}

/// Continuously answer probes. For simultaneous announcement handling, use serve.
pub async fn respond(
    socket: &UdpSocket<'_>,
    discovery: &Discovery,
    identity: polis_discovery_protocol::DeviceId,
) -> Result<(), Error<SendError>> {
    serve(socket, discovery, Some(identity), |_, _| {}).await
}
