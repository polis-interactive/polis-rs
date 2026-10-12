//! Runtime/link integration fixture, not a board application or hardware qualification.
#![no_std]
#![no_main]
use defmt_rtt as _;
use embassy_net::{Config, Ipv4Address, Ipv4Cidr, StackResources, StaticConfigV4, tcp::TcpSocket};
use embassy_net_driver_channel::{State as ChannelState, driver::HardwareAddress};
use embassy_rp::{
    bind_interrupts, peripherals,
    uart::{BufferedInterruptHandler, BufferedUart},
    usb,
};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use panic_probe as _;
use polis_control_link_embassy::{
    Buffers,
    protocol::{ControlLink, Message},
    serial, supervisor, tcp, uart,
};

// Minimal fixture behavior; production handlers provide their own handshake and queues.
struct UsbApplication;
impl<R: embedded_io_async::Read, W: embedded_io_async::Write> supervisor::ConnectionHandler<R, W>
    for UsbApplication
{
    async fn receive(&self, mut receiver: supervisor::Receiver<'_, '_, R>) {
        while receiver.receive(|_| ()).await.is_ok() {}
    }
    async fn transmit(&self, mut sender: supervisor::Sender<'_, '_, W>) {
        let _ = sender
            .send(Message::Event {
                session: core::num::NonZeroU64::new(1).unwrap(),
                body: b"link fixture",
            })
            .await;
        core::future::pending().await
    }
}

const LINK: ControlLink = ControlLink::new(*b"example-app-0001");
bind_interrupts!(struct Irqs {
    UART0_IRQ => BufferedInterruptHandler<peripherals::UART0>;
    USBCTRL_IRQ => usb::InterruptHandler<peripherals::USB>;
});
#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: embassy_executor::Spawner) {
    let p = embassy_rp::init(Default::default());
    let (mut uart_tx, mut uart_rx) = ([0; 256], [0; 256]);
    let uart = BufferedUart::new(
        p.UART0,
        p.PIN_0,
        p.PIN_1,
        Irqs,
        &mut uart_tx,
        &mut uart_rx,
        Default::default(),
    );
    let (writer, reader) = uart.split();
    let uart_task = async {
        let (mut rx, mut payload, mut tx) = ([0; 256], [0; 128], [0; 256]);
        let (mut sender, mut receiver) = uart::split(
            reader,
            writer,
            LINK,
            Buffers {
                receive: &mut rx,
                payload: &mut payload,
                transmit: &mut tx,
            },
        );
        loop {
            let key = receiver
                .receive(|m| match m {
                    Message::Request { key, .. } => Some(key),
                    _ => None,
                })
                .await
                .unwrap();
            if let Some(key) = key {
                sender
                    .send(Message::Response { key, body: b"uart" })
                    .await
                    .unwrap();
            }
        }
    };
    let driver = usb::Driver::new(p.USB, Irqs);
    let config = embassy_usb::Config::new(0xc0de, 0xcafe);
    let (mut descriptor, mut bos, mut msos, mut control) = ([0; 256], [0; 256], [0; 128], [0; 64]);
    let mut state = State::new();
    let mut builder = embassy_usb::Builder::new(
        driver,
        config,
        &mut descriptor,
        &mut bos,
        &mut msos,
        &mut control,
    );
    let class = CdcAcmClass::new(&mut builder, &mut state, 64);
    let mut usb = builder.build();
    let (mut usb_tx, mut usb_rx) = class.split();
    let serial_task = async {
        loop {
            usb_rx.wait_connection().await;
            let (mut rx, mut payload, mut tx, mut packet) = ([0; 256], [0; 128], [0; 256], [0; 64]);
            let pair = serial::split(
                &mut usb_tx,
                &mut usb_rx,
                LINK,
                Buffers {
                    receive: &mut rx,
                    payload: &mut payload,
                    transmit: &mut tx,
                },
                &mut packet,
            )
            .unwrap();
            supervisor::run(
                pair,
                embassy_time::Duration::from_millis(500),
                core::future::pending(), // Production USB reset notification is application-supplied.
                &UsbApplication,
            )
            .await;
        }
    };
    let mut channel_state = ChannelState::<1536, 2, 2>::new();
    let (mut channel, driver) = embassy_net_driver_channel::new(
        &mut channel_state,
        HardwareAddress::Ethernet([2, 0, 0, 0, 0, 1]),
    );
    let config = Config::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 1, 1), 24),
        gateway: None,
        dns_servers: Default::default(),
    });
    let mut resources = StackResources::<2>::new();
    let (stack, mut runner) = embassy_net::new(driver, config, &mut resources, 1);
    let tcp_task = async {
        let (mut net_rx, mut net_tx) = ([0; 512], [0; 512]);
        let mut socket = TcpSocket::new(stack, &mut net_rx, &mut net_tx);
        loop {
            socket.accept(9000).await.unwrap();
            let (mut rx, mut payload, mut tx) = ([0; 256], [0; 128], [0; 256]);
            let (mut sender, mut receiver) = tcp::split(
                &mut socket,
                LINK,
                Buffers {
                    receive: &mut rx,
                    payload: &mut payload,
                    transmit: &mut tx,
                },
            );
            while let Ok(Some(key)) = receiver
                .receive(|m| match m {
                    Message::Request { key, .. } => Some(key),
                    _ => None,
                })
                .await
            {
                if sender
                    .send(Message::Response { key, body: b"tcp" })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            socket.abort();
        }
    };
    let network = async {
        loop {
            channel.tx_buf().await;
            channel.tx_done();
        }
    };
    embassy_futures::join::join3(
        uart_task,
        embassy_futures::join::join(usb.run(), serial_task),
        embassy_futures::join::join3(tcp_task, runner.run(), network),
    )
    .await;
}
