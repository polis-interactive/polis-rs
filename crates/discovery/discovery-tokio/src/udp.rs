//! One framed discovery message per UDP datagram, using caller-configured sockets.
use std::{net::SocketAddr, num::NonZeroU64, time::Duration};

use polis_discovery_protocol::{Announcement, Discover, Discovery, Message};
use tokio::{net::UdpSocket, time::timeout};

use crate::{Error, WIRE_CAPACITY, decode, encode};

/// Common multicast destination. The caller selects interfaces and membership.
pub fn multicast_endpoint() -> SocketAddr {
    SocketAddr::from((
        polis_discovery_protocol::udp::DISCOVERY_MULTICAST_IPV4,
        polis_discovery_protocol::udp::DISCOVERY_PORT,
    ))
}

pub async fn send(
    socket: &UdpSocket,
    discovery: &Discovery,
    message: &Message,
    destination: SocketAddr,
) -> Result<(), Error<std::io::Error>> {
    let mut wire = [0; WIRE_CAPACITY];
    let len = encode(discovery, message, &mut wire)?;
    let sent = socket
        .send_to(&wire[..len], destination)
        .await
        .map_err(Error::Io)?;
    if sent != len {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::WriteZero,
            "partial discovery datagram",
        )));
    }
    Ok(())
}

/// Returns the source endpoint for replying/routing. Cancel-safe.
/// Rejects malformed, foreign-application and oversized datagrams individually.
pub async fn receive(
    socket: &UdpSocket,
    discovery: &Discovery,
) -> Result<(Message, SocketAddr), Error<std::io::Error>> {
    // One extra byte detects socket truncation even when a valid frame is at
    // the start of an oversized datagram. No 64 KiB receive allocation needed.
    let mut wire = [0; WIRE_CAPACITY + 1];
    let (len, peer) = socket.recv_from(&mut wire).await.map_err(Error::Io)?;
    let frame =
        polis_framing::decode_datagram(&mut wire[..len], WIRE_CAPACITY).map_err(Error::Framing)?;
    Ok((decode(discovery, frame)?, peer))
}

/// Send one probe and collect matching announcements for a fixed response window.
/// The send has its own window-length timeout. Replies are not deduplicated;
/// the caller owns its registry. Malformed/unrelated packets are ignored, while
/// socket errors propagate. Noise never extends the response deadline.
pub async fn discover(
    socket: &UdpSocket,
    discovery: &Discovery,
    id: NonZeroU64,
    destination: SocketAddr,
    window: Duration,
    mut on_device: impl FnMut(Announcement, SocketAddr),
) -> Result<(), Error<std::io::Error>> {
    let probe = Message::Discover(Discover {
        discovery_id: id.get(),
    });
    timeout(window, send(socket, discovery, &probe, destination))
        .await
        .map_err(|_| Error::TimedOut)??;
    let collect = async {
        loop {
            match receive(socket, discovery).await {
                Ok((Message::Announcement(value), peer)) if value.discovery_id == id.get() => {
                    on_device(value, peer)
                }
                Err(error @ Error::Io(_)) => return Err(error),
                _ => {}
            }
        }
    };
    // Expiration is a completed scan, including when no devices responded.
    timeout(window, collect).await.unwrap_or(Ok(()))
}

/// Send one unsolicited announcement. Call again at the application's chosen
/// interval, or only at startup; this function owns no background task.
pub async fn announce(
    socket: &UdpSocket,
    discovery: &Discovery,
    device_id: polis_discovery_protocol::DeviceId,
    destination: SocketAddr,
) -> Result<(), Error<std::io::Error>> {
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
    socket: &UdpSocket,
    discovery: &Discovery,
    identity: Option<polis_discovery_protocol::DeviceId>,
    mut on_device: impl FnMut(Announcement, SocketAddr),
) -> Result<(), Error<std::io::Error>> {
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
            Err(error @ Error::Io(_)) => return Err(error),
            _ => {}
        }
    }
}

/// Passively listen for announcements, without sending probes or replies.
pub async fn listen(
    socket: &UdpSocket,
    discovery: &Discovery,
    on_device: impl FnMut(Announcement, SocketAddr),
) -> Result<(), Error<std::io::Error>> {
    serve(socket, discovery, None, on_device).await
}

/// Continuously answer probes. For simultaneous announcement handling, use serve.
pub async fn respond(
    socket: &UdpSocket,
    discovery: &Discovery,
    identity: polis_discovery_protocol::DeviceId,
) -> Result<(), Error<std::io::Error>> {
    serve(socket, discovery, Some(identity), |_, _| {}).await
}
