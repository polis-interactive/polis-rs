//! Use a connected or accepted TCP socket; both peers have the same control API.
use crate::{Buffers, protocol::ControlLink, stream};
use tokio::net::{
    TcpStream,
    tcp::{OwnedReadHalf, OwnedWriteHalf},
};
pub type Link<'a> = stream::Pair<'a, OwnedReadHalf, OwnedWriteHalf>;

/// Configure low-latency messages and retain independent read/write halves.
/// Connect/accept addresses, timeouts and socket policy belong to the caller.
pub fn split(
    socket: TcpStream,
    link: ControlLink,
    buffers: Buffers<'_>,
) -> std::io::Result<Link<'_>> {
    socket.set_nodelay(true)?;
    let (reader, writer) = socket.into_split();
    Ok(stream::split(link, reader, writer, buffers))
}
