#![cfg(feature = "udp")]

use embassy_net::{
    Config, IpAddress, IpEndpoint, Ipv4Address, Ipv4Cidr, StackResources, StaticConfigV4,
    udp::{PacketMetadata, UdpSocket},
};
use embassy_net_driver_channel::{
    RxRunner, State, TxRunner,
    driver::{HardwareAddress, LinkState},
};
use polis_discovery_embassy::{Error, udp};
use polis_discovery_protocol::{Announcement, ApplicationId, DeviceId, Discovery, Message};
use std::num::NonZeroU64;

const CODEC: Discovery = Discovery::new(ApplicationId(*b"example-app-0001"));

fn config(last: u8) -> Config {
    Config::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 1, last), 24),
        gateway: None,
        dns_servers: Default::default(),
    })
}

async fn forward(mut from: TxRunner<'_, 1536>, mut to: RxRunner<'_, 1536>) {
    loop {
        let packet = from.tx_buf().await;
        let len = packet.len();
        to.rx_buf().await[..len].copy_from_slice(packet);
        to.rx_done(len);
        from.tx_done();
    }
}

#[tokio::test]
async fn real_embassy_stacks_exchange_discovery_and_reject_truncation() {
    // Two real Embassy/smoltcp stacks joined by an in-memory Ethernet cable.
    let mut a_state = State::<1536, 4, 4>::new();
    let mut b_state = State::<1536, 4, 4>::new();
    let (mut a_channel, a_driver) = embassy_net_driver_channel::new(
        &mut a_state,
        HardwareAddress::Ethernet([2, 0, 0, 0, 0, 1]),
    );
    let (mut b_channel, b_driver) = embassy_net_driver_channel::new(
        &mut b_state,
        HardwareAddress::Ethernet([2, 0, 0, 0, 0, 2]),
    );
    a_channel.set_link_state(LinkState::Up);
    b_channel.set_link_state(LinkState::Up);
    let (_, a_rx, a_tx) = a_channel.split();
    let (_, b_rx, b_tx) = b_channel.split();
    let mut a_resources = StackResources::<2>::new();
    let mut b_resources = StackResources::<2>::new();
    let (a_stack, mut a_runner) = embassy_net::new(a_driver, config(1), &mut a_resources, 1);
    let (b_stack, mut b_runner) = embassy_net::new(b_driver, config(2), &mut b_resources, 2);
    let (mut a_rx_meta, mut a_tx_meta) = ([PacketMetadata::EMPTY; 4], [PacketMetadata::EMPTY; 4]);
    let (mut b_rx_meta, mut b_tx_meta) = ([PacketMetadata::EMPTY; 4], [PacketMetadata::EMPTY; 4]);
    let (mut a_rx_buf, mut a_tx_buf) = ([0; 1024], [0; 1024]);
    let (mut b_rx_buf, mut b_tx_buf) = ([0; 1024], [0; 1024]);
    let mut host = UdpSocket::new(
        a_stack,
        &mut a_rx_meta,
        &mut a_rx_buf,
        &mut a_tx_meta,
        &mut a_tx_buf,
    );
    let mut device = UdpSocket::new(
        b_stack,
        &mut b_rx_meta,
        &mut b_rx_buf,
        &mut b_tx_meta,
        &mut b_tx_buf,
    );
    host.bind(40001).unwrap();
    device.bind(43000).unwrap();
    let destination = IpEndpoint::new(IpAddress::v4(192, 168, 1, 2), 43000);
    let host_address = IpEndpoint::new(IpAddress::v4(192, 168, 1, 1), 40001);

    let scenario = async {
        let responder = async {
            let (query, peer) = udp::receive(&device, &CODEC).await.unwrap();
            assert_eq!(peer, host_address);
            assert!(matches!(query, Message::Discover(_)));
            for id in [0, 6, 7, 8] {
                let message = Message::Announcement(Announcement {
                    discovery_id: id,
                    device_id: DeviceId(*b"device01"),
                });
                udp::send(&device, &CODEC, &message, peer).await.unwrap();
            }
        };
        let mut found = Vec::new();
        let scan = udp::discover(
            &host,
            &CODEC,
            NonZeroU64::new(7).unwrap(),
            destination,
            embassy_time::Duration::from_millis(40),
            |value, peer| found.push((value, peer)),
        );
        let (result, ()) = tokio::join!(scan, responder);
        result.unwrap();
        let reply = Announcement {
            discovery_id: 7,
            device_id: DeviceId(*b"device01"),
        };
        assert_eq!(found, [(reply, destination)]);

        device.send_to(&[0; 256], host_address).await.unwrap();
        assert!(matches!(
            udp::receive(&host, &CODEC).await,
            Err(Error::Io(embassy_net::udp::RecvError::Truncated))
        ));
        device.send_to(b"garbage", host_address).await.unwrap();
        assert!(matches!(
            udp::receive(&host, &CODEC).await,
            Err(Error::Framing(_))
        ));
        udp::send(
            &device,
            &Discovery::new(ApplicationId([0; 16])),
            &Message::Announcement(reply),
            host_address,
        )
        .await
        .unwrap();
        assert!(matches!(
            udp::receive(&host, &CODEC).await,
            Err(Error::Protocol(
                polis_discovery_protocol::Error::WrongApplication(_)
            ))
        ));
        udp::send(&device, &CODEC, &Message::Announcement(reply), host_address)
            .await
            .unwrap();
        assert_eq!(
            udp::receive(&host, &CODEC).await.unwrap(),
            (Message::Announcement(reply), destination)
        );
        // One receive loop can both register arrivals and answer probes.
        let mut arrivals = Vec::new();
        let client = async {
            udp::announce(&host, &CODEC, DeviceId(*b"host0001"), destination)
                .await
                .unwrap();
            let mut replies = Vec::new();
            udp::discover(
                &host,
                &CODEC,
                NonZeroU64::new(99).unwrap(),
                destination,
                embassy_time::Duration::from_millis(40),
                |a, peer| replies.push((a, peer)),
            )
            .await
            .unwrap();
            assert_eq!(
                replies,
                [(
                    Announcement {
                        discovery_id: 99,
                        device_id: DeviceId(*b"device01")
                    },
                    destination
                )]
            );
        };
        tokio::select! {
            result = udp::serve(&device, &CODEC, Some(DeviceId(*b"device01")), |a, peer| arrivals.push((a, peer))) => panic!("server stopped: {result:?}"),
            () = client => {},
        }
        assert_eq!(
            arrivals,
            [(
                Announcement::unsolicited(DeviceId(*b"host0001")),
                host_address
            )]
        );
        let mut passive = Vec::new();
        tokio::select! {
            result = udp::listen(&host, &CODEC, |a, peer| passive.push((a, peer))) => panic!("listener stopped: {result:?}"),
            () = async {
                udp::announce(&device, &CODEC, DeviceId(*b"device01"), host_address).await.unwrap();
                embassy_time::Timer::after_millis(20).await;
            } => {},
        }
        assert_eq!(
            passive,
            [(
                Announcement::unsolicited(DeviceId(*b"device01")),
                destination
            )]
        );
    };
    tokio::select! {
        _ = a_runner.run() => panic!("network runner stopped"),
        _ = b_runner.run() => panic!("network runner stopped"),
        _ = forward(a_tx, b_rx) => panic!("cable stopped"),
        _ = forward(b_tx, a_rx) => panic!("cable stopped"),
        result = tokio::time::timeout(std::time::Duration::from_secs(3), scenario) => result.unwrap(),
    }
}
