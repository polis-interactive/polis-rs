//! Complete USB CDC connection lifecycle (`serial` + `supervisor`).
//! Register State::handler with the USB builder, then hand the device and class
//! to Runner. No application connection task, readiness loop or reset signal.
use crate::{
    Buffers,
    protocol::ControlLink,
    serial::{self, PacketReader, PacketWriter},
    supervisor::{self, ConnectionHandler},
};
use core::{
    cell::{Cell, RefCell},
    future::poll_fn,
    task::{Poll, Waker},
};
use embassy_futures::{
    join::join,
    select::{Either, select},
};
use embassy_time::{Duration, Timer};
use embassy_usb::{UsbDevice, class::cdc_acm::CdcAcmClass, driver::Driver};

/// Product callbacks. They are synchronous so cancellation cannot interrupt
/// cleanup. All run in the runner's task; no Send/Sync or allocator is required.
pub trait Lifecycle {
    /// Called on USB reset, disable or unconfiguration, including while waiting
    /// for readiness. Invalidate product ownership/queued work here if needed.
    fn reset(&self) {}
    /// Both endpoints are ready. Prepare fresh per-connection application state.
    fn connected(&self) {}
    /// Called exactly once per connected() after both loops/halves are dropped,
    /// including cancellation of the entire runner. Discard stale queued work.
    fn disconnected(&self, _reason: Disconnect) {}
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disconnect {
    Link(supervisor::Exit),
    Cancelled,
}

/// Application-selected timing and codec. A reconnect delay prevents a failed
/// but still-enabled interface from spinning. No commands are retained/retried.
#[derive(Clone, Copy)]
pub struct Config {
    pub link: ControlLink,
    pub send_timeout: Duration,
    pub reconnect_delay: Duration,
}
pub struct Storage<'a> {
    pub frames: Buffers<'a>,
    pub packet: &'a mut [u8],
}

/// Latched USB reset state shared by the builder handler and runner. One runner
/// per State; keep it in the same task as the USB device. A reset followed by
/// immediate reconfiguration cannot be lost between endpoint polls.
#[derive(Default)]
pub struct State {
    generation: Cell<u64>,
    waker: RefCell<Option<Waker>>,
}
impl State {
    pub const fn new() -> Self {
        Self {
            generation: Cell::new(0),
            waker: RefCell::new(None),
        }
    }
    /// Register the returned handler with builder.handler(). Use this same State
    /// and application for Runner::new. USB descriptors remain application-owned.
    pub fn handler<'a, H: Lifecycle>(&'a self, application: &'a H) -> Events<'a, H> {
        Events {
            state: self,
            application,
        }
    }
    fn invalidate(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        let waker = self.waker.borrow_mut().take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
    async fn changed(&self, generation: u64) {
        struct Registration<'a>(&'a State);
        impl Drop for Registration<'_> {
            fn drop(&mut self) {
                self.0.waker.borrow_mut().take();
            }
        }
        let _registration = Registration(self);
        poll_fn(|cx| {
            if self.generation.get() != generation {
                Poll::Ready(())
            } else {
                self.waker.borrow_mut().replace(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}
/// USB event bridge created by State::handler. Product code need not implement
/// embassy_usb::Handler or maintain a separate reset signal/generation latch.
pub struct Events<'a, H> {
    state: &'a State,
    application: &'a H,
}
impl<H: Lifecycle> Events<'_, H> {
    fn invalidate(&self) {
        self.state.invalidate();
        self.application.reset();
    }
}
impl<H: Lifecycle> embassy_usb::Handler for Events<'_, H> {
    fn enabled(&mut self, enabled: bool) {
        if !enabled {
            self.invalidate();
        }
    }
    fn reset(&mut self) {
        self.invalidate();
    }
    fn configured(&mut self, configured: bool) {
        if !configured {
            self.invalidate();
        }
    }
}

/// Owns the USB device, CDC class, readiness/reconnect loop and supervision.
/// Use run alongside other firmware futures; do not spawn a separate USB task.
/// Each connection gets fresh framing adapters. Queues and fresh wire-session
/// negotiation remain in the application's ConnectionHandler/Lifecycle methods.
pub struct Runner<'a, 'd, D: Driver<'d>, H> {
    device: UsbDevice<'d, D>,
    sender: embassy_usb::class::cdc_acm::Sender<'d, D>,
    receiver: embassy_usb::class::cdc_acm::Receiver<'d, D>,
    started: bool,
    state: &'a State,
    application: &'a H,
    config: Config,
    storage: Storage<'a>,
}
impl<'a, 'd, D: Driver<'d>, H> Runner<'a, 'd, D, H>
where
    H: Lifecycle + for<'s> ConnectionHandler<PacketReader<'s, 'd, D>, PacketWriter<'s, 'd, D>>,
{
    /// Validate packet capacity before any USB I/O or lifecycle callback.
    pub fn new(
        device: UsbDevice<'d, D>,
        class: CdcAcmClass<'d, D>,
        state: &'a State,
        application: &'a H,
        config: Config,
        storage: Storage<'a>,
    ) -> Result<Self, serial::PacketBufferTooSmall> {
        if storage.packet.len() < usize::from(class.max_packet_size()) {
            return Err(serial::PacketBufferTooSmall);
        }
        let (sender, receiver) = class.split();
        Ok(Self {
            device,
            sender,
            receiver,
            started: false,
            state,
            application,
            config,
            storage,
        })
    }
    /// Run USB and reconnect indefinitely. Cancellation drops both application
    /// loops/adapter halves before invoking disconnected(Cancelled). No in-flight
    /// command is saved for the next connection. Backends must support cancellation.
    pub async fn run(&mut self) -> ! {
        // Embassy requires disable after cancellation of UsbDevice::run before
        // reuse. Remember this across cancellation, including during disable.
        if self.started {
            self.disable().await;
        }
        self.started = true;
        let storage = Storage {
            frames: Buffers {
                receive: self.storage.frames.receive,
                payload: self.storage.frames.payload,
                transmit: self.storage.frames.transmit,
            },
            packet: self.storage.packet,
        };
        match select(
            self.device.run(),
            connections(
                &mut self.sender,
                &mut self.receiver,
                self.state,
                self.application,
                self.config,
                storage,
            ),
        )
        .await
        {
            Either::First(never) => never,
            Either::Second(never) => never,
        }
    }
    /// After cancelling run(), disable USB before releasing/reusing hardware.
    /// Restarting run() does this automatically before waiting for a new connection.
    pub async fn disable(&mut self) {
        self.device.disable().await;
        self.started = false;
    }
}

struct Connected<'a, H: Lifecycle> {
    application: &'a H,
    reason: Disconnect,
}
impl<H: Lifecycle> Drop for Connected<'_, H> {
    fn drop(&mut self) {
        self.application.disconnected(self.reason);
    }
}
async fn connections<'d, D: Driver<'d>, H>(
    sender: &mut embassy_usb::class::cdc_acm::Sender<'d, D>,
    receiver: &mut embassy_usb::class::cdc_acm::Receiver<'d, D>,
    state: &State,
    application: &H,
    config: Config,
    storage: Storage<'_>,
) -> !
where
    H: Lifecycle + for<'s> ConnectionHandler<PacketReader<'s, 'd, D>, PacketWriter<'s, 'd, D>>,
{
    loop {
        let generation = state.generation.get();
        if matches!(
            select(
                state.changed(generation),
                join(receiver.wait_connection(), sender.wait_connection())
            )
            .await,
            Either::First(())
        ) {
            continue;
        }
        application.connected();
        // Declared before the supervised future: cancellation drops I/O first,
        // then product cleanup. A normal exit supplies the actual retirement cause.
        {
            let mut connected = Connected {
                application,
                reason: Disconnect::Cancelled,
            };
            let pair = serial::split(
                sender,
                receiver,
                config.link,
                Buffers {
                    receive: storage.frames.receive,
                    payload: storage.frames.payload,
                    transmit: storage.frames.transmit,
                },
                storage.packet,
            )
            .expect("packet capacity validated by Runner::new");
            let reason = supervisor::run(
                pair,
                config.send_timeout,
                state.changed(generation),
                application,
            )
            .await;
            connected.reason = Disconnect::Link(reason);
        }
        // Always yield, even if the application selects a zero reconnect delay.
        embassy_futures::yield_now().await;
        Timer::after(config.reconnect_delay).await;
    }
}
