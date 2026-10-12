//! A complete discovery probe/reply over two local UDP sockets.
use std::{num::NonZeroU64, time::Duration};

use polis_discovery_protocol::{ApplicationId, DeviceId, Discovery};
use polis_discovery_tokio::udp;
use tokio::net::UdpSocket;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let discovery = Discovery::new(ApplicationId(*b"example-app-0001"));
    let host = UdpSocket::bind("127.0.0.1:0").await?;
    let device = UdpSocket::bind("127.0.0.1:0").await?;
    let destination = device.local_addr()?;
    let responder = tokio::spawn(async move {
        udp::respond(&device, &discovery, DeviceId(*b"device01"))
            .await
            .unwrap();
    });
    udp::discover(
        &host,
        &discovery,
        NonZeroU64::new(1).unwrap(),
        destination,
        Duration::from_millis(100),
        |device, peer| {
            println!("discovered {:?} at {peer}", device.device_id);
        },
    )
    .await?;
    responder.abort();
    let _ = responder.await;
    Ok(())
}
