//! Host serial ports, including USB CDC. Port selection/settings are caller-owned.
use crate::{Buffers, protocol::ControlLink, stream};
use tokio::io::{ReadHalf, WriteHalf};
use tokio_serial::{SerialPortBuilder, SerialStream};
pub type Link<'a> = stream::Pair<'a, ReadHalf<SerialStream>, WriteHalf<SerialStream>>;

pub fn open<'a>(
    builder: &SerialPortBuilder,
    link: ControlLink,
    buffers: Buffers<'a>,
) -> tokio_serial::Result<Link<'a>> {
    Ok(split(SerialStream::open(builder)?, link, buffers))
}

/// An already-open port also supports application-specific modem control setup.
pub fn split(port: SerialStream, link: ControlLink, buffers: Buffers<'_>) -> Link<'_> {
    let (reader, writer) = tokio::io::split(port);
    stream::split(link, reader, writer, buffers)
}
