# Tokio discovery

`discovery-tokio` provides UDP endpoint discovery and RS-485 bus discovery.
UDP is enabled by default; disable defaults for RS-485 only.

Point-to-point UART and USB/host serial connections have no discovery adapter.
Their connection startup and session handshake belong to control-link. That
runtime implementation is a separate next step.

## Features

| Configuration | Cargo dependency |
| --- | --- |
| UDP | `discovery-tokio = { version = "0.1.0", default-features = false, features = ["udp"] }` |
| RS-485 | `discovery-tokio = { version = "0.1.0", default-features = false, features = ["rs485"] }` |
| Both | `discovery-tokio = { version = "0.1.0", default-features = false, features = ["udp", "rs485"] }` |
| No transport | `discovery-tokio = { version = "0.1.0", default-features = false }` |

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

## UDP

`udp::send` and `udp::receive` transfer one framed message per datagram.
`udp::discover` sends a probe, then collects announcements matching its nonzero
discovery ID for a fixed window. Each reply includes the sender's source endpoint.
The callback maintains the device registry and deduplicates announcements.
Invalid or unrelated packets cannot extend the window. Socket errors propagate.
The send phase has a separate timeout equal to the response-window duration.

Run a complete local probe/reply example:

```sh
cargo run --locked -p discovery-tokio --example udp
```

See [examples/udp.rs](examples/udp.rs) for both roles. For network deployment,
bind a device socket to the discovery port and join the group from
`udp::multicast_endpoint()` on the chosen interface. A scanning host can use an
ephemeral source port: devices reply to the endpoint returned by `receive`, not
an assumed discovery port. The caller owns multicast membership, interface
selection, broadcast configuration, retries, and periodic unsolicited announcements.
The default group/port comes from `discovery-protocol`; overrides are ordinary
socket destinations. Discovery does not select the subsequent control TCP port;
the application supplies its service endpoint policy.

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
Embassy can use this repository's `rs485::Rs485` driver over a wire-complete UART.
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
Tokio tests use local UDP sockets and a simulated RS-485 backend with a paused
clock. This package is host-only; physical RS-485 backends are not qualified.

```sh
python3 tools/check.py test --package discovery-tokio
```
