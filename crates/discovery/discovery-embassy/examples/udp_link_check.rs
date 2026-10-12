//! UDP link fixture using an in-memory Ethernet driver.
//! This validates runtime/linker integration, not physical Wi-Fi connectivity.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_net::{
    Config, Ipv4Address, Ipv4Cidr, StackResources, StaticConfigV4,
    udp::{PacketMetadata, UdpSocket},
};
use embassy_net_driver_channel::{
    State,
    driver::{HardwareAddress, LinkState},
};
use panic_probe as _;
use polis_discovery_embassy::udp;
use polis_discovery_protocol::{ApplicationId, Discovery};

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let _p = embassy_rp::init(Default::default());
    let discovery = Discovery::new(ApplicationId(*b"example-app-0001"));
    let mut channel_state = State::<1536, 2, 2>::new();
    let (mut channel, driver) = embassy_net_driver_channel::new(
        &mut channel_state,
        HardwareAddress::Ethernet([2, 0, 0, 0, 0, 1]),
    );
    channel.set_link_state(LinkState::Up);
    let mut resources = StackResources::<2>::new();
    let config = Config::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 1, 1), 24),
        gateway: None,
        dns_servers: Default::default(),
    });
    let (stack, mut runner) = embassy_net::new(driver, config, &mut resources, 1);
    stack
        .join_multicast_group(udp::multicast_endpoint().addr)
        .unwrap();
    let (mut rx_meta, mut tx_meta) = ([PacketMetadata::EMPTY; 2], [PacketMetadata::EMPTY; 2]);
    let (mut rx_buf, mut tx_buf) = ([0; 256], [0; 256]);
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    socket
        .bind(polis_discovery_protocol::udp::DISCOVERY_PORT)
        .unwrap();
    let udp_task = async {
        loop {
            udp::discover(
                &socket,
                &discovery,
                core::num::NonZeroU64::new(1).unwrap(),
                udp::multicast_endpoint(),
                embassy_time::Duration::from_secs(1),
                |_, _| {},
            )
            .await
            .unwrap();
        }
    };
    // Consume Ethernet output; a real application supplies its radio/Ethernet driver.
    let ethernet = async {
        loop {
            channel.tx_buf().await;
            channel.tx_done();
        }
    };
    embassy_futures::join::join3(udp_task, runner.run(), ethernet).await;
}
