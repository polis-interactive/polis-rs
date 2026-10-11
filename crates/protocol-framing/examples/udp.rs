//! Exactly one complete frame per UDP datagram on loopback.
use std::{error::Error, net::UdpSocket, time::Duration};

use framing::{Header, decode_datagram, encode_message_datagram, wire_capacity};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Message<'a> {
    sequence: u32,
    #[serde(borrow)]
    data: &'a [u8],
}

fn main() -> Result<(), Box<dyn Error>> {
    let sender = UdpSocket::bind("127.0.0.1:0")?;
    let receiver = UdpSocket::bind("127.0.0.1:0")?;
    // This example uses one peer per socket. UDP decoding holds no stream state.
    sender.connect(receiver.local_addr()?)?;
    receiver.connect(sender.local_addr()?)?;
    receiver.set_read_timeout(Some(Duration::from_secs(2)))?;

    let data: Vec<u8> = (0..768).map(|n| n as u8).collect();
    let message = Message {
        sequence: 42,
        data: &data,
    };
    let header = Header {
        protocol_id: 1,
        schema_version: 1,
        message_type: 1,
    };
    let mut scratch = [0; 1024];
    let mut tx = [0; wire_capacity(1024)];
    // This is a path-specific UDP payload budget, not the application payload
    // size. It includes the complete encoded frame and excludes IP/UDP headers.
    let budget = 1200;
    let wire = encode_message_datagram(header, &message, budget, &mut scratch, &mut tx)?;
    assert_eq!(sender.send(wire)?, wire.len());

    // Accommodate a full UDP datagram so recv cannot silently truncate one.
    // Embedded adapters may use their stack's explicit truncation reporting.
    let mut datagram = [0; 65535];
    let len = receiver.recv(&mut datagram)?;
    let frame = decode_datagram(&mut datagram[..len], budget)?;
    assert_eq!(frame.header, header);
    assert_eq!(frame.deserialize::<Message<'_>>()?, message);
    println!(
        "UDP: one {}-byte frame in one datagram (limit {budget} bytes)",
        wire.len()
    );
    Ok(())
}
