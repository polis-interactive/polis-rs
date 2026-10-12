# polis-control-link-embassy

Allocation-free, `no_std` point-to-point control communication. Both peers use
the same send/receive API. One connection carries the identity/session handshake,
requests, replies and unsolicited events, with independent read and write halves.

| Feature | Transport | Default |
| --- | --- | --- |
| `uart` | MCU hardware UART through `embedded-io-async` | Yes |
| `tcp` | Connected/accepted `embassy-net` TCP socket; Wi-Fi or Ethernet | No |
| `serial` | USB CDC ACM using `embassy-usb` | No |

Features are independently optional. `stream` always supports generic
`embedded-io-async` read/write halves. The library selects no MCU, driver,
executor, network configuration, USB descriptors, pins or baud rate. There is
no discovery dependency or RS-485 bus protocol.

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
application-owned. Neither direction blocks the other through a shared lock.

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
