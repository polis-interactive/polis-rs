# Embassy discovery

`polis-discovery-embassy` provides UDP endpoint discovery and RS-485 bus discovery.
No transport is enabled by default; select UDP, RS-485, or both.

Point-to-point UART and USB/host serial connections have no discovery adapter.
Their connection startup and session handshake belong to control-link. That
runtime implementation is a separate next step.

## Features

| Configuration | Cargo dependency |
| --- | --- |
| UDP | `polis-discovery-embassy = { version = "0.1.0", default-features = false, features = ["udp"] }` |
| RS-485 | `polis-discovery-embassy = { version = "0.1.0", default-features = false, features = ["rs485"] }` |
| Both | `polis-discovery-embassy = { version = "0.1.0", default-features = false, features = ["udp", "rs485"] }` |
| No transport | `polis-discovery-embassy = { version = "0.1.0", default-features = false }` |

Normal library builds select no hardware, executor or time driver. UDP enables
Embassy Net/Time; RS-485 enables embedded I/O and Embassy Time. Buffers are bounded,
with no allocation or `std`. The application supplies configured sockets/buses,
identities, and a time driver.

## Discovery roles

Application server/client labels do not select discovery behavior or the later
TCP connection direction. Choose the behavior on either peer:

| Behavior | UDP API |
| --- | --- |
| Announce “I'm here” | `udp::announce` once, or on the application's timer |
| Passively register arrivals | `udp::listen` with an announcement callback |
| Probe and collect replies | `udp::discover` with a fixed response window |
| Answer probes | `udp::respond` with this peer's identity |
| Register arrivals and answer probes | `udp::serve` with an identity and callback |

Only one receive loop may own a socket. Do not run `discover` alongside `serve`
on the same socket; use a separate probing socket when continuously listening.
Malformed/foreign messages are ignored by these role loops. The application owns
its registry, announce cadence and the subsequent TCP endpoint/connection policy.

## UDP / Wi-Fi

Create/bind an Embassy `UdpSocket` on a running stack and pass it to
`udp::send`, `udp::receive`, or `udp::discover`. These work over an IPv4
Ethernet/Wi-Fi stack regardless of the underlying radio. Radio initialization,
association, DHCP/static configuration and credentials remain application-owned.
Use `udp::multicast_endpoint()` for the shared default and join that group on
the chosen stack. A device replies to the source endpoint returned by `receive`.
The subsequent control endpoint/port remains application policy.

```rust,no_run
# #[cfg(feature = "udp")]
# async fn respond(socket: &embassy_net::udp::UdpSocket<'_>) {
use polis_discovery_protocol::{Announcement, ApplicationId, DeviceId, Discovery, Message};
use polis_discovery_embassy::udp;

let discovery = Discovery::new(ApplicationId(*b"example-app-0001"));
let (message, peer) = udp::receive(socket, &discovery).await.unwrap();
if let Message::Discover(request) = message {
    let reply = Message::Announcement(Announcement {
        discovery_id: request.discovery_id,
        device_id: DeviceId(*b"device01"), // application supplies actual identity
    });
    udp::send(socket, &discovery, &reply, peer).await.unwrap();
}
# }
```

Allocate packet metadata and at least `WIRE_CAPACITY` bytes per queued discovery
datagram for the socket. Embassy rejects receive truncation; the adapter then
validates that the datagram contains exactly one frame. `discover` bounds its
send phase and then collects matching announcements for a fixed window. It
ignores malformed/unrelated datagrams; the callback owns deduplication. Retries,
periodic announcements and registry retention remain explicit caller choices.

## RS-485

The optional `rs485` module implements scheduled half-duplex discovery:

- `discover::<N, _>` sends one round, retains up to N distinct correlated replies,
  waits for the entire response window, then sends ACKs. Its callback updates the
  application's registry. Overflowed identities are not ACKed; retry with the same
  session and a new seed to discover remaining peers.
- `serve` owns a participant's receive loop, schedules its response slot, consumes
  ACKs, and dispatches discovery/other protocols to the application callback.
  `respond` handles one already-decoded message for an application-owned loop.
- `Participant` retains ACK suppression and the last handled seed. A new session
  re-enables replies. Ordinary Discover messages never grant RS-485 bus access.

Supply a direction-controlled bus whose successful write/flush includes the final
stop bit and bus release. A raw UART or ordinary host `SerialStream` is insufficient.
Embassy can use this repository's `polis_rs485::Rs485` driver over a wire-complete UART.
Tokio requires a backend that supplies equivalent direction/completion behavior.
Recover the driver after a failed/cancelled send before further I/O. Reads used in
timed controller collection must be cancel-safe, or explicitly discard/recover
lost input before reusing the bus.

`Timing` contains two measured bounds: `receive_margin_us` covers either early RX
notification or late buffered delivery relative to the final stop bit;
`transmit_budget_us` covers sending the whole response and releasing the bus.
Include propagation from the controller, clock resolution/drift and scheduling
jitter in the bounds. The adapter narrows each slot by
the receive margin on both ends and skips replies that cannot still fit. This
cannot guarantee timing on an OS/USB path with unbounded latency. Discard stale RX
bytes before starting a participant, so the timestamp bound holds from startup.

Exactly one controller owns each physical bus. Other traffic waits until discovery
finishes. If a controller scan is cancelled after sending its round, keep the bus
quiet for the full response window before transmitting again. Retries, session/seed
generation, bus configuration and control address assignment remain explicit caller
choices. These helpers do not infer electrical timing from a baud setting.

## Validation

The build tool covers four feature combinations: none, UDP, RS-485, and both.
Tests cover UDP arrival/probe roles, malformed traffic and fixed deadlines;
RS-485 tests cover slots, ACK suppression, bounded collection and timeouts.
Embassy tests use in-memory Ethernet and the host time driver. MCU builds cover
RP2040 and RP2350, with two linked fixtures:

- `udp_link_check`: UDP plus the network runner, using simulated Ethernet.
- `rs485_link_check`: discovery through the direction driver and PIO UART
  (GP18 TX, GP20 RX, GP19 DE/RE at 1 Mbaud and a 128 MHz system clock).

Fixtures use example-only configuration. They are built, not flashed; physical
networking, bus timing and collisions still require hardware validation.

```sh
python3 tools/check.py build --package polis-discovery-embassy --platform all
```

Use `udp,rs485` for both transports. Do not use `--all-features`, which also
selects mutually exclusive MCU features. `link-check` enables fixture dependencies;
select exactly one of `rp2040` and `rp235xa` when building a fixture directly.
