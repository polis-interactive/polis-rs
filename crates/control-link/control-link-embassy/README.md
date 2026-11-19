# polis-control-link-embassy

Allocation-free, `no_std` point-to-point control communication. Both peers use
the same send/receive API. One connection carries the identity/session handshake,
requests, replies and events. Full-duplex transports use independent read/write
halves; half-duplex transports use one exclusively owned link with application-granted turns.

| Feature | Transport | Default |
| --- | --- | --- |
| `uart` | MCU hardware UART through `embedded-io-async` | Yes |
| `tcp` | Connected/accepted `embassy-net` TCP socket; Wi-Fi or Ethernet | No |
| `serial` | USB CDC ACM using `embassy-usb` | No |

Features are independently optional. `stream` always supports generic
`embedded-io-async` read/write halves; `half_duplex` always supports one generic
`Read + Write` transport, including a configured `polis-rs485` driver. Neither
module requires a transport feature or adds a driver/runtime dependency.
The library selects no MCU, driver,
executor, network configuration, USB descriptors, pins or baud rate. There is
no discovery dependency, multidrop addressing or bus arbitration protocol.

## Half-duplex RS-485

Use `half_duplex::Link::new(transport, codec, buffers)` with one ready transport.
It owns the same persistent framing state used by `stream`; wire bytes and the
existing full-duplex APIs are unchanged. With `polis-rs485`, call `recover()` before
construction. The UART backend must provide physical final-stop-bit completion;
`polis-pio-uart` provides that contract. Configure DE polarity and transceiver
delays in board setup.

```rust,no_run
# async fn example<S: embedded_io_async::Read + embedded_io_async::Write>(
#     ready_bus: S, fresh_session: core::num::NonZeroU64,
# ) -> Result<bool, polis_control_link_embassy::Error<S::Error>> {
use polis_control_link_embassy::{
    Buffers, half_duplex::Link,
    protocol::{ControlLink, Incoming, Message, Requests},
};
let (mut rx, mut payload, mut tx) = ([0; 160], [0; 128], [0; 160]);
let mut link = Link::new(ready_bus, ControlLink::new(*b"example-app-0001"), Buffers {
    receive: &mut rx, payload: &mut payload, transmit: &mut tx,
});
let mut requests = Requests::<1>::new(fresh_session);
let key = requests.reserve().unwrap();
// Application has granted this node the bus. Add deadlines around send/receive.
link.send(Message::Request { key, body: b"hello-v1" }).await?;
let identity_matches = link.receive(|message| {
    matches!(requests.receive(message),
        Ok(Incoming::Response { body: b"example-peer-v1", .. }))
}).await?;
// Retain this SAME link and tracker for normal traffic after validating identity.
# Ok(identity_matches)
# }
```

Both methods require `&mut self`: a single task sends and receives in sequence.
There are no split halves or background tasks, and `supervisor::run` is for the
full-duplex APIs only. The application owns bus turns, bounded reply windows,
queues, handshake/session validation, `Requests`/`RequestGate`, and reconnects.
For a fixed two-node pair, a simple policy is one controller request followed by
one responder reply. Unsolicited Events also require a granted transmit window.
Discovery is unnecessary. Holding a mutex across an unbounded receive would
block local commands and is not a substitute for scheduling.

`send` encodes a complete frame before I/O and flushes before returning. Through
`Rs485`, one frame uses one DE assertion, including all underlying short UART
writes, physical completion and receive settling. Generic backends must provide
equivalent direction/completion semantics. Successful transmission does not prove
execution; match a terminal Response and interpret its application body.

Cancelling or failing a send after encoding retires both directions; subsequent
calls return `Error::Interrupted`. Codec errors before I/O leave the link usable.
Receive I/O errors or EOF close both directions. Framing/protocol errors are
reported individually and allow continued decoding. Cancelling receive retains
partial frames and unread bytes, provided the backend's pending read is itself
cancellation-safe. A receive deadline does not automatically retire the link,
cancel a request, clear buffered replies or grant a bus turn. Enforce a bounded
reply window at both peers and reject stale/late replies with the trackers.

For recovery, consume the link with `into_inner()`, then run `bus.recover()` with
a deadline. This drains stale TX with DE inactive; it does not purge backend RX.
Construct a replacement link and negotiate a fresh session after invalidating
uncertain work. No commands are retained or replayed. An unanswered request can
already have executed; physical recovery alone does not resolve its outcome.

### Two-board example

[`examples/half_duplex.rs`](examples/half_duplex.rs) runs on Pico or Pico 2 with
one THVD1420 transceiver per board. On **each board** connect GP18 to DI, GP20 to
RO, and GP19 to tied DE and /RE. Connect the transceivers' A/B bus and common
ground, with appropriate termination/biasing and a DE pull-down during reset.
Ground GP22 on the **initiator only**; leave it open on the responder (internal
pull-up). Exactly one board must initiate. These are example pins and THVD1420
timings, not the HC board configuration.

The same image on both boards uses PIO1 SM0/SM1 at 128 MHz, 1 Mbaud 8N1. Start
the responder first. The initiator waits one second, generates a fresh demo
session nonce, exchanges a versioned identity and sends `ping` every 100 ms;
matched `pong` replies appear in initiator RTT output. The responder validates
request ordering, waits 1 ms for turnaround, and refuses to start a reply more
than 5 ms after parsing its request. Sends have a 5 ms deadline and the initiator
allows 20 ms for each reply. These illustrative budgets require a cooperative
executor; they are not measured latency guarantees or a product safety policy.
This is an identity/I/O demo, not authentication or motor control.

Any transport failure or initiator reply timeout ends the demo, attempts physical
recovery, and stops without retransmission. Reset both boards to restart. There
is no product reconnect/authorization policy. PIO RX still has only an eight-byte
FIFO and no overflow reporting; the adapter adds no background RX ring. Logging
is kept outside active reply windows. Bench-check receive capacity and DE/stop-bit
timing before operational use.

```sh
cargo build -p polis-control-link-embassy --no-default-features --example half_duplex --features example-rp2040 --target thumbv6m-none-eabi --release --locked
cargo build -p polis-control-link-embassy --no-default-features --example half_duplex --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

Run the matching image with an SWD probe:

```sh
probe-rs run --chip RP2040 .target/thumbv6m-none-eabi/release/examples/half_duplex
probe-rs run --chip RP235x .target/thumbv8m.main-none-eabihf/release/examples/half_duplex
```

Host tests cover paired request/reply exchange, wire compatibility, short I/O,
coalesced/partial/malformed frames, stale/duplicate messages, lost-response policy,
and cancellation at each RS-485 send/recovery stage. Linked examples validate
runtime integration; they do not qualify electrical behavior or STOP latency.

## UART and TCP

Split a configured HAL UART and pass its **reader, writer** to `uart::split`.
HALs may return the halves in the opposite order. For TCP, connect or accept a
socket before calling `tcp::split(&mut socket, ...)`, and run the network stack
alongside your control tasks. Wi-Fi credentials and the radio driver remain in
the application.

```rust,no_run
#[cfg(feature = "uart")]
async fn example<R: embedded_io_async::Read, W: embedded_io_async::Write>(reader: R, writer: W) {
    use polis_control_link_embassy::{
        Buffers,
        protocol::{ControlLink, Message, RequestGate},
        uart,
    };
    use core::num::NonZeroU64;
    let link = ControlLink::new(*b"example-app-0001");
    let (mut receive, mut payload, mut transmit) = ([0; 320], [0; 284], [0; 320]);
    let (mut sender, mut receiver) = uart::split(
        reader,
        writer,
        link,
        Buffers {
            receive: &mut receive,
            payload: &mut payload,
            transmit: &mut transmit,
        },
    );
    // Illustrative only: use the fresh session accepted by your identity handshake.
    let mut gate = RequestGate::new(NonZeroU64::new(1).unwrap());
    let request = receiver
        .receive(
            |message| -> Result<_, polis_control_link_embassy::protocol::StateError> {
                match message {
                    Message::Request { key, body } => {
                        gate.accept(key)?; // Before invoking business logic.
                        // Deserialize/copy body into an application value for async work.
                        let _ = body;
                        Ok(Some(key))
                    }
                    _ => Ok(None), // Route responses/events through Requests in a bidirectional application.
                }
            },
        )
        .await
        .unwrap()
        .unwrap();
    if let Some(key) = request {
        sender
            .send(Message::Response { key, body: b"done" })
            .await
            .unwrap();
    }
}
```

The synchronous receive callback borrows the body. Return an owned application
value to handle asynchronously. Every call returns one message and retains
unread/coalesced frames. Keep the same receiver throughout the handshake: no
serial discovery, connection upgrade, decoder reset or buffered-byte handoff.

## USB CDC serial

For a complete connection lifecycle, enable `serial` and `supervisor` and use
`usb::Runner`. It owns the USB device run future, CDC splitting, endpoint readiness
waits, reset detection, RX/TX supervision and reconnection. No application USB
connection task or reconnect loop is required.

Build descriptors and the CDC class in board setup. Create a `usb::State` and
register `state.handler(&application)` with the USB builder. The returned handler
latches reset, disable and unconfiguration events, including a reset followed by
reconfiguration between endpoint polls. Pass the **same state and application**
to the runner. Implement `ConnectionHandler` for business receive/transmit loops
and `usb::Lifecycle` for synchronous product callbacks:

- `reset()` invalidates product ownership immediately, even while awaiting readiness.
- `connected()` prepares fresh application session state once both endpoints are ready.
- `disconnected(reason)` runs once per connection, after both adapters and business
  futures are dropped, including cancellation of the runner.

```rust,no_run
# #[cfg(all(feature = "serial", feature = "supervisor"))]
# async fn example<'a, 'd, D, H>(device: embassy_usb::UsbDevice<'d, D>,
#     class: embassy_usb::class::cdc_acm::CdcAcmClass<'d, D>,
#     state: &'a polis_control_link_embassy::usb::State, application: &'a H,
#     storage: polis_control_link_embassy::usb::Storage<'a>)
#     -> Result<(), polis_control_link_embassy::serial::PacketBufferTooSmall>
# where D: embassy_usb::driver::Driver<'d>,
# H: polis_control_link_embassy::usb::Lifecycle + for<'s> polis_control_link_embassy::supervisor::ConnectionHandler<
#     polis_control_link_embassy::serial::PacketReader<'s, 'd, D>,
#     polis_control_link_embassy::serial::PacketWriter<'s, 'd, D>> {
use polis_control_link_embassy::{protocol::ControlLink, usb};
use embassy_time::Duration;
let mut runner = usb::Runner::new(device, class, state, application, usb::Config {
    link: ControlLink::new(*b"example-app-0001"),
    send_timeout: Duration::from_millis(500),
    reconnect_delay: Duration::from_millis(1),
}, storage)?;
runner.run().await // Run alongside the rest of the firmware, e.g. with join().
# }
```

`usb::Storage` contains the existing `Buffers` plus packet storage of at least
`class.max_packet_size()` bytes. Construction checks packet capacity. Readiness
means interface enable; any additional DTR/identity policy belongs in the handler.
All callbacks and futures run in one task with no allocation or `Send` requirement.
The application owns queue filtering, fresh wire-session negotiation, timeout and
reconnect-delay values, and power-off policy. The runner never stores or replays
commands; a new connection always gets fresh framing/packet adapters.

Dropping `runner.run()` cancels both business loops before calling
`disconnected(Cancelled)`. Embassy requires disabling a cancelled USB device
before reuse: calling `runner.run()` again does that automatically. If stopping
permanently, call `runner.disable().await` before releasing the hardware. Drop
cannot perform an async peripheral shutdown. Reset/disconnect of an active link
does not cancel the device's USB event loop; reconnection remains inside the runner.

The low-level `serial::split` API remains available for consumers owning their
own lifecycle. It chunks writes at endpoint packet size, sends a zero-length
packet when flushing an exact packet multiple, and ignores incoming zero-length
packets. USB descriptors, packet sizes and board pins are never selected by the library.

## Sessions, buffers and cancellation

Compose `protocol::Requests<N>` for outgoing requests with a `RequestGate` for
incoming requests. The application chooses a fresh nonzero session, defines its
identity payload/validation, and retains the trackers through the handshake.
The transport layer does not authenticate or assign session IDs. Response/event
handlers, deadlines, tasks, bounded queues, reconnect policy and keepalives are
application-owned. With full-duplex split APIs, neither direction blocks the
other through a shared lock. Half-duplex operations are deliberately exclusive.

For body limit B <= `MAX_BODY_LEN`, payload storage needs `PREFIX_LEN + B` bytes;
receive/transmit each need `polis_framing::wire_capacity((PREFIX_LEN + B) as u16)` bytes.
The receiver also retains a 256-byte I/O chunk. No heap is required.

Receive state survives cancellation, but cancelling a pending backend read is
safe **only if that driver guarantees it**. A cancelled or failed send retires
the sender: subsequent calls return `Error::Interrupted`. Retire both halves and
start a fresh session, without replaying a possibly executed request. Codec
errors happen before I/O and leave the sender usable. Only a matched Response
confirms a command outcome. EOF/I/O errors close the receiver; framing/protocol
errors are surfaced individually. `into_inner` recovers generic backends when
retiring a link and discards buffered input.

The `link_check` example links real RP UART and USB drivers plus an in-memory
Ethernet stack on RP2040/RP2350. Its echo traffic exercises I/O only; it is not a
product identity/authorization policy. It does not validate physical hardware.

## Optional connection supervisor

Enable `supervisor` to give both halves one connection lifetime. The existing
low-level `stream`, `uart`, `serial` and `tcp` split APIs are unchanged. This
feature adds Embassy futures/time, stays `no_std`, and allocates nothing.

Implement `supervisor::ConnectionHandler<R, W>` on your application context.
Its `receive` and `transmit` methods take `&self` and run concurrently in one
task, so they can share channels/interior state without a `Send` requirement.
Each gets an owned, scoped adapter; neither adapter can escape the connection.
Keep the receive decoder through the identity handshake just as with split.

```rust,no_run
# #[cfg(feature = "supervisor")]
# async fn example<R: embedded_io_async::Read, W: embedded_io_async::Write>(
#     pair: polis_control_link_embassy::stream::Pair<'_, R, W>,
#     reset_or_generation_changed: impl core::future::Future<Output = ()>,
#     application: impl polis_control_link_embassy::supervisor::ConnectionHandler<R, W>,
# ) {
use embassy_time::Duration;
use polis_control_link_embassy::supervisor;
let reason = supervisor::run(
    pair, // From serial::split, uart::split, tcp::split or stream::split.
    Duration::from_millis(500), // Application policy, not a library default.
    reset_or_generation_changed,
    &application,
).await;
// Both adapter halves and application loops have been dropped here.
// Invalidate application queues/session, then wait for transport readiness and reconnect.
let _ = reason;
# }
```

A receive I/O error/EOF, any send error, send timeout (including flush), cancelled
send, USB reset notification, or either loop returning retires both halves.
Dropping `run` itself also drops both halves. Application drop guards still run,
including accounting for an unfinished outgoing message. `Exit` reports why the
connection ended; detailed errors remain available to the application loop.
Malformed/foreign received frames remain recoverable for filtering/counters;
truncated EOF is fatal. This is stricter than low-level send, whose pre-I/O codec
errors are retryable. Callbacks must yield cooperatively, as with other Embassy
code. Cancelling a read still requires a cancellation-safe backend.

The supervisor performs no reconnect/retry by itself and stores no commands.
The application reborrows/reopens its transport and calls `run` with a fresh pair.
It must also filter/invalidate old queued work and establish a fresh session.
For USB, supply a **latched reset or generation-change future**, not just endpoint
read errors: reset and reconfiguration can happen between endpoint polls. An
already-ready reset wins before any I/O. USB interface readiness and DTR policy
stay with the caller; after transport failure an endpoint can still be enabled.

The wired-control firmware's `usb.rs` contains only `ConnectionHandler` business
loops, lifecycle cleanup and loss accounting. Its link task, USB device task,
reset handler, reset signal and reconnect loop were deleted. Board setup hands
the USB device/class, configuration, buffers and application to `usb::Runner`.
The RP link fixture uses the same complete runner.
