//! TCP over any embassy-net interface, including Wi-Fi and Ethernet.
use crate::{Buffers, protocol::ControlLink, stream};
use embassy_net::tcp::{TcpReader, TcpSocket, TcpWriter};
pub type Link<'a, 's> = stream::Pair<'a, TcpReader<'s>, TcpWriter<'s>>;

/// Call after socket.connect() or socket.accept(). Keep the network runner alive.
pub fn split<'a, 's>(
    socket: &'s mut TcpSocket<'_>,
    link: ControlLink,
    buffers: Buffers<'a>,
) -> Link<'a, 's> {
    let (reader, writer) = socket.split();
    stream::split(link, reader, writer, buffers)
}
