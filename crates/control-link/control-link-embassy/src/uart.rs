//! MCU hardware UART. Configure and split the HAL driver before constructing a link.
//! Pins, baud rate and buffering belong to the board. No UART driver or MCU is selected here.
use crate::{Buffers, protocol::ControlLink, stream};
use embedded_io_async::{Read, Write};

pub fn split<R: Read, W: Write>(
    reader: R,
    writer: W,
    link: ControlLink,
    buffers: Buffers<'_>,
) -> stream::Pair<'_, R, W> {
    stream::split(link, reader, writer, buffers)
}
