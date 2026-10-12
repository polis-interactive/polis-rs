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

Build the CDC class and USB device in the application. Split the class into its
sender/receiver and wait for interface enable. Pass mutable references to those
halves, `ControlLink`, `Buffers`, and a separate packet buffer to `serial::split`.
The packet buffer must hold `receiver.max_packet_size()` bytes (64 for full-speed
CDC); construction checks its size. Keep the USB device runner alive.

The adapter chunks writes at endpoint packet size, sends a zero-length packet
when flushing an exact packet multiple, and ignores incoming zero-length packets.
Disconnect errors propagate; they are not stream EOF. Drop both link halves on
disconnect, wait for the next connection using the original CDC halves, and
establish a fresh session. `wait_connection` means interface enabled; any DTR
policy is application-owned.

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

The wired-control firmware's `usb.rs` now implements its loops on an `Application`
handler. It replaces the manual RX/TX/reset race with `supervisor::run` and uses
`sender.send(...)` without a separate `with_timeout`. Its generation counter,
priority queues, pending-frame loss accounting, handshake and power-off policy
remain application-owned. The RP link fixture also implements the handler trait.
