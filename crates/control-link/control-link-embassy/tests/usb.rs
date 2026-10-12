#![cfg(all(feature = "serial", feature = "supervisor"))]
use embassy_usb::{
    class::cdc_acm::{CdcAcmClass, State},
    driver::*,
};
use polis_control_link_embassy::{
    Buffers,
    protocol::{ControlLink, Message},
    supervisor, usb,
};
use std::{cell::RefCell, collections::VecDeque, num::NonZeroU64, rc::Rc};
const LINK: ControlLink = ControlLink::new(*b"example-app-0001");
#[derive(Default)]
struct Packets {
    ready: bool,
    reads: usize,
    disables: usize,
    limit: Option<usize>,
    events: VecDeque<Event>,
    setups: VecDeque<[u8; 8]>,
    waker: Option<core::task::Waker>,
    incoming: VecDeque<Vec<u8>>,
    sent: Vec<Vec<u8>>,
}
impl Packets {
    fn wake(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
    fn configure(&mut self) {
        self.setups.push_back([0, 9, 1, 0, 0, 0, 0, 0]);
        self.wake();
    }
}
struct MockDriver(Rc<RefCell<Packets>>, u8);
struct MockEndpoint {
    info: EndpointInfo,
    packets: Rc<RefCell<Packets>>,
}
impl<'a> Driver<'a> for MockDriver {
    type EndpointOut = MockEndpoint;
    type EndpointIn = MockEndpoint;
    type ControlPipe = MockControl;
    type Bus = MockBus;
    fn alloc_endpoint_out(
        &mut self,
        ep_type: EndpointType,
        ep_addr: Option<EndpointAddress>,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<MockEndpoint, EndpointAllocError> {
        Ok(MockEndpoint {
            info: EndpointInfo {
                addr: ep_addr.unwrap_or(EndpointAddress::from(1)),
                ep_type,
                max_packet_size,
                interval_ms,
            },
            packets: self.0.clone(),
        })
    }
    fn alloc_endpoint_in(
        &mut self,
        ep_type: EndpointType,
        ep_addr: Option<EndpointAddress>,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<MockEndpoint, EndpointAllocError> {
        self.1 += 1;
        Ok(MockEndpoint {
            info: EndpointInfo {
                addr: ep_addr.unwrap_or(EndpointAddress::from(0x80 | self.1)),
                ep_type,
                max_packet_size,
                interval_ms,
            },
            packets: self.0.clone(),
        })
    }
    fn start(self, _: u16) -> (MockBus, MockControl) {
        (MockBus(self.0.clone()), MockControl(self.0))
    }
}
impl Endpoint for MockEndpoint {
    fn info(&self) -> &EndpointInfo {
        &self.info
    }
    async fn wait_enabled(&mut self) {
        core::future::poll_fn(|cx| {
            let mut packets = self.packets.borrow_mut();
            if packets.ready {
                core::task::Poll::Ready(())
            } else {
                packets.waker = Some(cx.waker().clone());
                core::task::Poll::Pending
            }
        })
        .await
    }
}
impl EndpointOut for MockEndpoint {
    async fn read(&mut self, out: &mut [u8]) -> Result<usize, EndpointError> {
        self.packets.borrow_mut().reads += 1;
        core::future::poll_fn(|cx| {
            let mut packets = self.packets.borrow_mut();
            if !packets.ready {
                return core::task::Poll::Ready(Err(EndpointError::Disabled));
            }
            if let Some(packet) = packets.incoming.pop_front() {
                out[..packet.len()].copy_from_slice(&packet);
                core::task::Poll::Ready(Ok(packet.len()))
            } else {
                packets.waker = Some(cx.waker().clone());
                core::task::Poll::Pending
            }
        })
        .await
    }
}
impl EndpointIn for MockEndpoint {
    async fn write(&mut self, bytes: &[u8]) -> Result<(), EndpointError> {
        core::future::poll_fn(|cx| {
            let mut packets = self.packets.borrow_mut();
            if !packets.ready {
                return core::task::Poll::Ready(Err(EndpointError::Disabled));
            }
            if packets
                .limit
                .is_some_and(|limit| packets.sent.len() >= limit)
            {
                packets.waker = Some(cx.waker().clone());
                core::task::Poll::Pending
            } else {
                packets.sent.push(bytes.to_vec());
                core::task::Poll::Ready(Ok(()))
            }
        })
        .await
    }
}
struct MockBus(Rc<RefCell<Packets>>);
impl Bus for MockBus {
    async fn enable(&mut self) {}
    async fn disable(&mut self) {
        let mut packets = self.0.borrow_mut();
        packets.disables += 1;
        packets.ready = false;
        packets.wake();
    }
    async fn poll(&mut self) -> Event {
        core::future::poll_fn(|cx| {
            let mut packets = self.0.borrow_mut();
            if let Some(event) = packets.events.pop_front() {
                if matches!(event, Event::Reset | Event::PowerRemoved) {
                    packets.ready = false;
                    packets.incoming.clear();
                }
                core::task::Poll::Ready(event)
            } else {
                packets.waker = Some(cx.waker().clone());
                core::task::Poll::Pending
            }
        })
        .await
    }
    fn endpoint_set_enabled(&mut self, _: EndpointAddress, enabled: bool) {
        self.0.borrow_mut().ready = enabled;
    }
    fn endpoint_set_stalled(&mut self, _: EndpointAddress, _: bool) {}
    fn endpoint_is_stalled(&mut self, _: EndpointAddress) -> bool {
        false
    }
    async fn remote_wakeup(&mut self) -> Result<(), Unsupported> {
        Ok(())
    }
}
struct MockControl(Rc<RefCell<Packets>>);
impl ControlPipe for MockControl {
    fn max_packet_size(&self) -> usize {
        64
    }
    async fn setup(&mut self) -> [u8; 8] {
        core::future::poll_fn(|cx| {
            let mut packets = self.0.borrow_mut();
            if let Some(setup) = packets.setups.pop_front() {
                core::task::Poll::Ready(setup)
            } else {
                packets.waker = Some(cx.waker().clone());
                core::task::Poll::Pending
            }
        })
        .await
    }
    async fn data_out(&mut self, _: &mut [u8], _: bool, _: bool) -> Result<usize, EndpointError> {
        unreachable!()
    }
    async fn data_in(&mut self, _: &[u8], _: bool, _: bool) -> Result<(), EndpointError> {
        unreachable!()
    }
    async fn accept(&mut self) {}
    async fn reject(&mut self) {
        panic!("unexpected control request")
    }
    async fn accept_set_address(&mut self, _: u8) {}
}
use core::{cell::Cell, future::pending};
use embassy_futures::select::{Either, select};
use embassy_time::Duration;
use embedded_io_async::{Read, Write};
#[derive(Default)]
struct Application {
    connected: Cell<u64>,
    resets: Cell<usize>,
    live: Cell<usize>,
    cancelled_frames: Cell<usize>,
    disconnected: RefCell<Vec<usb::Disconnect>>,
    received: RefCell<Vec<Vec<u8>>>,
    stop_transmit_after_first: bool,
}
impl usb::Lifecycle for Application {
    fn reset(&self) {
        self.resets.set(self.resets.get() + 1);
    }
    fn connected(&self) {
        assert_eq!(self.live.get(), 0);
        self.connected.set(self.connected.get() + 1);
    }
    fn disconnected(&self, reason: usb::Disconnect) {
        assert_eq!(
            self.live.get(),
            0,
            "business loops must drop before cleanup"
        );
        self.disconnected.borrow_mut().push(reason);
    }
}
struct Live<'a>(&'a Cell<usize>);
impl<'a> Live<'a> {
    fn new(live: &'a Cell<usize>) -> Self {
        live.set(live.get() + 1);
        Self(live)
    }
}
impl Drop for Live<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}
struct Frame<'a>(&'a Cell<usize>, bool);
impl Drop for Frame<'_> {
    fn drop(&mut self) {
        if !self.1 {
            self.0.set(self.0.get() + 1);
        }
    }
}
fn command(session: u64) -> Message<'static> {
    Message::Request {
        key: polis_control_link_embassy::protocol::Key {
            session: NonZeroU64::new(session).unwrap(),
            id: core::num::NonZeroU32::new(1).unwrap(),
        },
        body: &[42; 120],
    }
}
fn wire(message: Message<'_>) -> Vec<u8> {
    LINK.encode(message, &mut [0; 256], &mut [0; 300])
        .unwrap()
        .to_vec()
}
impl<R: Read, W: Write> supervisor::ConnectionHandler<R, W> for Application {
    async fn receive(&self, mut receiver: supervisor::Receiver<'_, '_, R>) {
        let _live = Live::new(&self.live);
        while receiver
            .receive(|m| self.received.borrow_mut().push(m.body().to_vec()))
            .await
            .is_ok()
        {}
    }
    async fn transmit(&self, mut sender: supervisor::Sender<'_, '_, W>) {
        let _live = Live::new(&self.live);
        if !self.stop_transmit_after_first || self.connected.get() == 1 {
            let mut frame = Frame(&self.cancelled_frames, false);
            if sender.send(command(self.connected.get())).await.is_err() {
                return;
            }
            frame.1 = true;
        }
        pending().await
    }
}
async fn until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn runner_owns_readiness_latched_reset_reconnection_and_cancelled_restart() {
    let packets = Rc::new(RefCell::new(Packets {
        limit: Some(1),
        ..Default::default()
    }));
    let application = Application::default();
    let reset_state = usb::State::new();
    let mut events = reset_state.handler(&application);
    let mut cdc = State::new();
    let (mut descriptor, mut bos, mut msos, mut control) = ([0; 256], [0; 256], [0; 128], [0; 64]);
    let mut builder = embassy_usb::Builder::new(
        MockDriver(packets.clone(), 0),
        embassy_usb::Config::new(0xc0de, 0xcafe),
        &mut descriptor,
        &mut bos,
        &mut msos,
        &mut control,
    );
    builder.handler(&mut events);
    let class = CdcAcmClass::new(&mut builder, &mut cdc, 64);
    let (mut rx, mut payload, mut tx, mut packet) = ([0; 300], [0; 256], [0; 300], [0; 64]);
    let mut runner = usb::Runner::new(
        builder.build(),
        class,
        &reset_state,
        &application,
        usb::Config {
            link: LINK,
            send_timeout: Duration::from_secs(10),
            reconnect_delay: Duration::from_millis(1),
        },
        usb::Storage {
            frames: Buffers {
                receive: &mut rx,
                payload: &mut payload,
                transmit: &mut tx,
            },
            packet: &mut packet,
        },
    )
    .unwrap();
    let scenario = async {
        for _ in 0..3 {
            tokio::task::yield_now().await;
        }
        assert_eq!(application.connected.get(), 0);
        assert_eq!(packets.borrow().reads, 0);
        packets.borrow_mut().configure();
        until(|| packets.borrow().sent.len() == 1).await;
        let old = wire(command(1));
        {
            let mut p = packets.borrow_mut();
            p.incoming.push_back(old[..10].to_vec());
            p.wake();
        }
        until(|| packets.borrow().reads >= 2).await;
        // Reset and reconfigure BEFORE the connection future polls again.
        {
            let mut p = packets.borrow_mut();
            p.events.push_back(Event::Reset);
            p.limit = None;
            p.configure();
        }
        until(|| application.connected.get() == 2 && packets.borrow().sent.len() >= 4).await;
        assert_eq!(
            *application.disconnected.borrow(),
            [usb::Disconnect::Link(supervisor::Exit::Reset)]
        );
        assert_eq!(application.cancelled_frames.get(), 1);
        let new = wire(command(2));
        assert_eq!(
            packets.borrow().sent.concat(),
            [old[..64].to_vec(), new].concat()
        );
        assert!(application.received.borrow().is_empty());
    };
    assert!(matches!(
        select(runner.run(), scenario).await,
        Either::Second(())
    ));
    assert_eq!(application.live.get(), 0);
    assert_eq!(
        application.disconnected.borrow().last(),
        Some(&usb::Disconnect::Cancelled)
    );
    assert_eq!(application.disconnected.borrow().len(), 2);
    // Restart the SAME runner. It must disable the cancelled USB device first.
    let scenario = async {
        until(|| packets.borrow().disables == 1).await;
        assert_eq!(application.connected.get(), 2);
        packets.borrow_mut().configure();
        until(|| application.connected.get() == 3 && packets.borrow().sent.len() >= 7).await;
    };
    assert!(matches!(
        select(runner.run(), scenario).await,
        Either::Second(())
    ));
    assert_eq!(application.disconnected.borrow().len(), 3);
    assert_eq!(application.live.get(), 0);
    runner.disable().await;
    assert_eq!(packets.borrow().disables, 2);
}

#[tokio::test]
async fn timeout_reconnects_enabled_usb_without_replaying_and_drops_partial_rx() {
    let packets = Rc::new(RefCell::new(Packets {
        limit: Some(1),
        ..Default::default()
    }));
    let application = Application {
        stop_transmit_after_first: true,
        ..Default::default()
    };
    let reset_state = usb::State::new();
    let mut events = reset_state.handler(&application);
    let mut cdc = State::new();
    let (mut descriptor, mut bos, mut msos, mut control) = ([0; 256], [0; 256], [0; 128], [0; 64]);
    let mut builder = embassy_usb::Builder::new(
        MockDriver(packets.clone(), 0),
        embassy_usb::Config::new(0xc0de, 0xcafe),
        &mut descriptor,
        &mut bos,
        &mut msos,
        &mut control,
    );
    builder.handler(&mut events);
    let class = CdcAcmClass::new(&mut builder, &mut cdc, 64);
    let (mut rx, mut payload, mut tx, mut packet) = ([0; 300], [0; 256], [0; 300], [0; 64]);
    let mut runner = usb::Runner::new(
        builder.build(),
        class,
        &reset_state,
        &application,
        usb::Config {
            link: LINK,
            send_timeout: Duration::from_millis(5),
            reconnect_delay: Duration::from_millis(1),
        },
        usb::Storage {
            frames: Buffers {
                receive: &mut rx,
                payload: &mut payload,
                transmit: &mut tx,
            },
            packet: &mut packet,
        },
    )
    .unwrap();
    {
        let mut p = packets.borrow_mut();
        p.incoming.push_back(wire(command(1))[..10].to_vec());
        p.configure();
    }
    let scenario = async {
        until(|| application.connected.get() == 2).await;
        assert_eq!(
            *application.disconnected.borrow(),
            [usb::Disconnect::Link(supervisor::Exit::SendTimedOut)]
        );
        assert_eq!(application.cancelled_frames.get(), 1);
        assert_eq!(packets.borrow().sent.concat(), wire(command(1))[..64]);
        let new = Message::Event {
            session: NonZeroU64::new(2).unwrap(),
            body: b"new session",
        };
        {
            let mut p = packets.borrow_mut();
            p.incoming.push_back(wire(new));
            p.wake();
        }
        until(|| !application.received.borrow().is_empty()).await;
        assert_eq!(*application.received.borrow(), [b"new session".to_vec()]);
        assert_eq!(packets.borrow().sent.len(), 1);
    };
    assert!(matches!(
        select(runner.run(), scenario).await,
        Either::Second(())
    ));
    assert_eq!(application.live.get(), 0);
    assert_eq!(application.disconnected.borrow().len(), 2);
    runner.disable().await;
}
