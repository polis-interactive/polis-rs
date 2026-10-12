#![cfg(feature = "serial")]
use embassy_usb::{
    class::cdc_acm::{CdcAcmClass, State},
    driver::*,
};
use polis_control_link_embassy::{
    Buffers, Error,
    protocol::{ControlLink, Message},
    serial,
};
use std::{cell::RefCell, collections::VecDeque, num::NonZeroU64, rc::Rc};
const LINK: ControlLink = ControlLink::new(*b"example-app-0001");
#[derive(Default)]
struct Packets {
    pending_when_empty: bool,
    reads: usize,
    incoming: VecDeque<Vec<u8>>,
    sent: Vec<Vec<u8>>,
}
struct MockDriver(Rc<RefCell<Packets>>, u8);
struct MockEndpoint {
    info: EndpointInfo,
    packets: Rc<RefCell<Packets>>,
}
impl<'a> Driver<'a> for MockDriver {
    type EndpointOut = MockEndpoint;
    type EndpointIn = MockEndpoint;
    type ControlPipe = Unused;
    type Bus = Unused;
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
    fn start(self, _: u16) -> (Unused, Unused) {
        (Unused, Unused)
    }
}
impl Endpoint for MockEndpoint {
    fn info(&self) -> &EndpointInfo {
        &self.info
    }
    async fn wait_enabled(&mut self) {}
}
impl EndpointOut for MockEndpoint {
    async fn read(&mut self, out: &mut [u8]) -> Result<usize, EndpointError> {
        self.packets.borrow_mut().reads += 1;
        let empty_pending = {
            let packets = self.packets.borrow();
            packets.pending_when_empty && packets.incoming.is_empty()
        };
        if empty_pending {
            return core::future::pending().await;
        }
        let packet = self
            .packets
            .borrow_mut()
            .incoming
            .pop_front()
            .ok_or(EndpointError::Disabled)?;
        assert!(out.len() >= usize::from(self.info.max_packet_size));
        out[..packet.len()].copy_from_slice(&packet);
        Ok(packet.len())
    }
}
impl EndpointIn for MockEndpoint {
    async fn write(&mut self, bytes: &[u8]) -> Result<(), EndpointError> {
        assert!(bytes.len() <= usize::from(self.info.max_packet_size));
        self.packets.borrow_mut().sent.push(bytes.to_vec());
        Ok(())
    }
}
struct Unused;
impl Bus for Unused {
    async fn enable(&mut self) {
        unreachable!()
    }
    async fn disable(&mut self) {
        unreachable!()
    }
    async fn poll(&mut self) -> Event {
        unreachable!()
    }
    fn endpoint_set_enabled(&mut self, _: EndpointAddress, _: bool) {
        unreachable!()
    }
    fn endpoint_set_stalled(&mut self, _: EndpointAddress, _: bool) {
        unreachable!()
    }
    fn endpoint_is_stalled(&mut self, _: EndpointAddress) -> bool {
        unreachable!()
    }
    async fn remote_wakeup(&mut self) -> Result<(), Unsupported> {
        unreachable!()
    }
}
impl ControlPipe for Unused {
    fn max_packet_size(&self) -> usize {
        64
    }
    async fn setup(&mut self) -> [u8; 8] {
        unreachable!()
    }
    async fn data_out(&mut self, _: &mut [u8], _: bool, _: bool) -> Result<usize, EndpointError> {
        unreachable!()
    }
    async fn data_in(&mut self, _: &[u8], _: bool, _: bool) -> Result<(), EndpointError> {
        unreachable!()
    }
    async fn accept(&mut self) {
        unreachable!()
    }
    async fn reject(&mut self) {
        unreachable!()
    }
    async fn accept_set_address(&mut self, _: u8) {
        unreachable!()
    }
}

#[tokio::test]
async fn cdc_handles_zlp_packet_boundaries_and_disconnect() {
    let packets = Rc::new(RefCell::new(Packets::default()));
    let (mut descriptor, mut bos, mut msos, mut control) = ([0; 256], [0; 256], [0; 128], [0; 64]);
    let mut state = State::new();
    let mut builder = embassy_usb::Builder::new(
        MockDriver(packets.clone(), 0),
        embassy_usb::Config::new(0xc0de, 0xcafe),
        &mut descriptor,
        &mut bos,
        &mut msos,
        &mut control,
    );
    let class = CdcAcmClass::new(&mut builder, &mut state, 64);
    let (mut usb_tx, mut usb_rx) = class.split();
    let _device = builder.build();
    let session = NonZeroU64::new(1).unwrap();
    // Exercise a complete encoded frame with an exact USB packet multiple.
    let body = (0..200)
        .map(|len| vec![42; len])
        .find(|body| {
            LINK.encode(
                Message::Event { session, body },
                &mut [0; 256],
                &mut [0; 300],
            )
            .unwrap()
            .len()
                == 128
        })
        .unwrap();
    let message = Message::Event {
        session,
        body: &body,
    };
    let frame = LINK
        .encode(message, &mut [0; 256], &mut [0; 300])
        .unwrap()
        .to_vec();
    packets.borrow_mut().incoming.push_back(vec![]);
    packets
        .borrow_mut()
        .incoming
        .extend(frame.chunks(64).map(<[u8]>::to_vec));
    packets.borrow_mut().incoming.push_back(vec![]);
    packets
        .borrow_mut()
        .incoming
        .extend(frame.chunks(64).map(<[u8]>::to_vec));
    let (mut rx, mut payload, mut tx, mut packet) = ([0; 300], [0; 256], [0; 300], [0; 64]);
    // Validate storage before touching endpoints.
    assert!(
        serial::split(
            &mut usb_tx,
            &mut usb_rx,
            LINK,
            Buffers {
                receive: &mut rx,
                payload: &mut payload,
                transmit: &mut tx
            },
            &mut packet[..63]
        )
        .is_err()
    );
    let (mut sender, mut receiver) = serial::split(
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
    receiver.receive(|m| assert_eq!(m, message)).await.unwrap();
    receiver.receive(|m| assert_eq!(m, message)).await.unwrap();
    sender.send(message).await.unwrap();
    {
        let sent = &packets.borrow().sent;
        assert_eq!(sent.iter().map(Vec::len).collect::<Vec<_>>(), [64, 64, 0]);
        assert_eq!(sent.concat(), frame);
    }
    assert!(matches!(
        receiver.receive(|_| ()).await,
        Err(Error::Io(EndpointError::Disabled))
    ));
    assert!(matches!(receiver.receive(|_| ()).await, Err(Error::Closed)));
}

#[cfg(feature = "supervisor")]
#[tokio::test]
async fn supervised_usb_reset_discards_partial_input_and_reborrows_cdc_for_reconnection() {
    use core::{
        future::{pending, poll_fn},
        task::Poll,
    };
    use embedded_io_async::{Read, Write};
    use polis_control_link_embassy::supervisor::{self, ConnectionHandler, Exit};
    struct Application(RefCell<Vec<Vec<u8>>>);
    impl<R: Read, W: Write> ConnectionHandler<R, W> for Application {
        async fn receive(&self, mut receiver: supervisor::Receiver<'_, '_, R>) {
            receiver
                .receive(|m| self.0.borrow_mut().push(m.body().to_vec()))
                .await
                .unwrap();
        }
        async fn transmit(&self, _: supervisor::Sender<'_, '_, W>) {
            pending().await
        }
    }
    let packets = Rc::new(RefCell::new(Packets {
        pending_when_empty: true,
        ..Default::default()
    }));
    let (mut descriptor, mut bos, mut msos, mut control) = ([0; 256], [0; 256], [0; 128], [0; 64]);
    let mut state = State::new();
    let mut builder = embassy_usb::Builder::new(
        MockDriver(packets.clone(), 0),
        embassy_usb::Config::new(0xc0de, 0xcafe),
        &mut descriptor,
        &mut bos,
        &mut msos,
        &mut control,
    );
    let class = CdcAcmClass::new(&mut builder, &mut state, 64);
    let (mut usb_tx, mut usb_rx) = class.split();
    let _device = builder.build();
    let (mut rx, mut payload, mut tx, mut packet) = ([0; 300], [0; 256], [0; 300], [0; 64]);
    let application = Application(RefCell::new(vec![]));
    let old = Message::Event {
        session: NonZeroU64::new(1).unwrap(),
        body: b"old partial",
    };
    let bytes = LINK
        .encode(old, &mut [0; 256], &mut [0; 300])
        .unwrap()
        .to_vec();
    packets
        .borrow_mut()
        .incoming
        .push_back(bytes[..10].to_vec());
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
    let reset = poll_fn(|cx| {
        if packets.borrow().reads >= 2 {
            Poll::Ready(())
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    });
    assert_eq!(
        supervisor::run(
            pair,
            embassy_time::Duration::from_secs(1),
            reset,
            &application
        )
        .await,
        Exit::Reset
    );
    assert!(application.0.borrow().is_empty());
    // Both CDC borrows have ended. A new connection uses the same endpoints and storage.
    usb_rx.wait_connection().await;
    usb_tx.wait_connection().await;
    let new = Message::Event {
        session: NonZeroU64::new(2).unwrap(),
        body: b"new connection",
    };
    let bytes = LINK
        .encode(new, &mut [0; 256], &mut [0; 300])
        .unwrap()
        .to_vec();
    packets
        .borrow_mut()
        .incoming
        .extend(bytes.chunks(64).map(<[u8]>::to_vec));
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
    assert_eq!(
        supervisor::run(
            pair,
            embassy_time::Duration::from_secs(1),
            pending(),
            &application
        )
        .await,
        Exit::ReceiveEnded
    );
    assert_eq!(*application.0.borrow(), [b"new connection".to_vec()]);
    assert!(packets.borrow().sent.is_empty());
}
