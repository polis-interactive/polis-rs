//! Framed typed messages over a native TCP byte stream.
use std::{
    error::Error,
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    thread,
    time::Duration,
};

use framing::{Decoder, Header, encode_message, wire_capacity};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Ping {
    sequence: u32,
}

fn main() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let header = Header {
        protocol_id: 1,
        schema_version: 1,
        message_type: 1,
    };
    let sender = thread::spawn(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut scratch = [0; 32];
        let mut tx = [0; wire_capacity(32)];
        for sequence in 0..3 {
            let wire = encode_message(header, &Ping { sequence }, &mut scratch, &mut tx).unwrap();
            for chunk in wire.chunks(7) {
                stream.write_all(chunk).unwrap();
            }
        }
        stream.shutdown(Shutdown::Write).unwrap();
    });
    let (mut stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut storage = [0; wire_capacity(32)];
    let mut decoder = Decoder::new(&mut storage);
    let mut read = [0; 17];
    let mut received = 0;
    loop {
        let len = stream.read(&mut read)?;
        if len == 0 {
            decoder.finish()?;
            break;
        }
        decoder.feed(&read[..len], |event| {
            let frame = event.expect("valid frame");
            assert_eq!(frame.header, header);
            assert_eq!(
                frame.deserialize::<Ping>().unwrap(),
                Ping { sequence: received }
            );
            received += 1;
        });
    }
    sender.join().unwrap();
    assert_eq!(received, 3);
    println!("TCP: {received} typed frames reconstructed across arbitrary reads");
    Ok(())
}
