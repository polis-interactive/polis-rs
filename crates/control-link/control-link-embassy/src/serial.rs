//! USB CDC serial. The application builds/runs USB and waits for interface enable.
//! Reborrow the CDC halves with fresh buffers/session after disconnect. Interface
//! enable and DTR are different; application policy decides whether DTR is required.
use crate::{Buffers, protocol::ControlLink, stream};
use embassy_usb::{
    class::cdc_acm::{Receiver, Sender},
    driver::{Driver, EndpointError},
};
use embedded_io_async::{ErrorType, Read, Write};

pub type Link<'a, 'd, D> = stream::Pair<'a, PacketReader<'a, 'd, D>, PacketWriter<'a, 'd, D>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PacketBufferTooSmall;
impl core::fmt::Display for PacketBufferTooSmall {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("USB packet buffer must hold the endpoint's maximum packet size")
    }
}
impl core::error::Error for PacketBufferTooSmall {}

/// Packet storage must hold receiver.max_packet_size() bytes (64 for full speed).
/// Keep the same link through the handshake and subsequent control traffic.
pub fn split<'a, 'd, D: Driver<'d>>(
    sender: &'a mut Sender<'d, D>,
    receiver: &'a mut Receiver<'d, D>,
    link: ControlLink,
    buffers: Buffers<'a>,
    packet: &'a mut [u8],
) -> Result<Link<'a, 'd, D>, PacketBufferTooSmall> {
    if packet.len() < usize::from(receiver.max_packet_size()) {
        return Err(PacketBufferTooSmall);
    }
    Ok(stream::split(
        link,
        PacketReader {
            receiver,
            packet,
            cursor: 0,
            len: 0,
        },
        PacketWriter {
            sender,
            needs_zlp: false,
        },
        buffers,
    ))
}

/// Adapts CDC packet reads to a stream, retaining leftovers and ignoring USB ZLPs.
pub struct PacketReader<'a, 'd, D: Driver<'d>> {
    receiver: &'a mut Receiver<'d, D>,
    packet: &'a mut [u8],
    cursor: usize,
    len: usize,
}
impl<'d, D: Driver<'d>> ErrorType for PacketReader<'_, 'd, D> {
    type Error = EndpointError;
}
impl<'d, D: Driver<'d>> Read for PacketReader<'_, 'd, D> {
    async fn read(&mut self, out: &mut [u8]) -> Result<usize, Self::Error> {
        if out.is_empty() {
            return Ok(0);
        }
        while self.cursor == self.len {
            self.len = self.receiver.read_packet(self.packet).await?;
            self.cursor = 0;
        }
        let n = out.len().min(self.len - self.cursor);
        out[..n].copy_from_slice(&self.packet[self.cursor..self.cursor + n]);
        self.cursor += n;
        Ok(n)
    }
}

/// Packetizes writes; flush sends a ZLP after an exact multiple of packet size.
pub struct PacketWriter<'a, 'd, D: Driver<'d>> {
    sender: &'a mut Sender<'d, D>,
    needs_zlp: bool,
}
impl<'d, D: Driver<'d>> ErrorType for PacketWriter<'_, 'd, D> {
    type Error = EndpointError;
}
impl<'d, D: Driver<'d>> Write for PacketWriter<'_, 'd, D> {
    async fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let max = usize::from(self.sender.max_packet_size());
        let n = bytes.len().min(max);
        self.sender.write_packet(&bytes[..n]).await?;
        self.needs_zlp = n == max;
        Ok(n)
    }
    async fn flush(&mut self) -> Result<(), Self::Error> {
        if self.needs_zlp {
            self.sender.write_packet(&[]).await?;
            self.needs_zlp = false;
        }
        Ok(())
    }
}
