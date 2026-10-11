# Control link protocol

`control-link-protocol` provides an allocation-free `no_std` message envelope
and request tracking for host/controller communication. It uses `framing` on
serial and TCP alike. Bodies are application-owned bytes; handlers can use
Postcard or another encoding without adding product types to this crate.

```rust
use core::num::NonZeroU64;
use control_link_protocol::{ControlLink, Incoming, Message, Requests};

let codec = ControlLink::new(*b"example-app-0001");
// Example only: the connection owner generates a fresh nonce for each session.
let mut requests = Requests::<2>::new(NonZeroU64::new(7).unwrap());
let key = requests.reserve()?;
let mut scratch = [0; 64];
let mut wire = [0; framing::wire_capacity(64)];
let bytes = codec.encode(Message::Request { key, body: b"read-status" },
                         &mut scratch, &mut wire)?;
assert_eq!(bytes[0], 0);

// An adapter delivers decoded messages while the request remains pending.
assert_eq!(requests.receive(Message::Event {
    session: key.session, body: b"telemetry",
})?, Incoming::Event { body: b"telemetry" });
requests.receive(Message::Accepted { key })?; // optional, still pending
requests.receive(Message::Response { key, body: b"done" })?;
assert_eq!(requests.pending_count(), 0);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Composition

An adapter owns the socket/UART/USB stream, a `framing::Decoder`, and the async
read/write loop. Dispatch complete frames by protocol ID: discovery frames go
to `discovery-protocol`, control frames to `ControlLink::decode`. Both codecs use
the same application-assigned 16 bytes. There is no production dependency
between the two protocols. Direct connections need no discovery.

Route decoded Requests to the application's handler after `RequestGate::accept`.
The handler may send Accepted, then a final Response, and publish Events at any
time. Successful and failed business outcomes are encoded in the response body.
Route replies and events through `Requests::receive`; a terminal response frees
its request slot. Copy or decode borrowed bodies before feeding the decoder
again. Async handlers need caller-owned storage for anything retained across reads.

## Session and delivery rules

The connection owner establishes the session through its application's handshake
and identity checks, and generates a fresh nonzero nonce on reconnection. These
checks and the initial Hello payload remain application-defined. A RequestGate
starts after session acceptance; pass the Hello key through it too if Hello is
an ordinary Request. Keep the gate for that session so repeated handshakes cannot
erase its high-water mark. Nonces must not be reused across reconnects/restarts.

Reserve keys and transmit requests in order. The receiver rejects duplicate or
decreasing IDs before business invocation; gaps are allowed. IDs never wrap.
Use a fresh session after exhaustion. Each peer may originate requests with its
own tracker and gate; replies are matched within that direction. Two outstanding
slots suffice for one long operation and an independent keepalive request.

`cancel` retires the local slot only. It neither retracts bytes already sent nor
stops the peer's operation. Runtime adapters must distinguish a request never
sent from one whose outcome is uncertain; never automatically replay the latter.
Dropping a session fails its pending callers. Old-session events and responses
are rejected. I/O belongs to runtime adapters. Applications compose timers, queues, reconnects
and keepalive scheduling as needed; this crate does not spawn tasks or choose
timeout values.

## Schema 1

The shared framing header uses protocol ID `0x5043`, schema version `1`.
These are local assignments, not registered service identifiers. Every payload
starts with 16 application bytes, a nonzero little-endian u64 session, and a
little-endian u32 request ID. The body is the remaining bytes, without another
length prefix. Maximum body size is 65,507 bytes; adapters should use smaller,
explicit memory budgets. Framing's declared length and CRC cover the full payload.

| Message ID | Meaning | Request ID | Body |
| --- | --- | --- | --- |
| 1 | Request | Nonzero | Application command |
| 2 | Accepted | Request being accepted | Empty |
| 3 | Response | Request being completed | Application result |
| 4 | Event | Zero | Application event |

Use `PREFIX_LEN + maximum_body_size` for the framing payload budget and
`framing::wire_capacity(budget)` for wire and decoder buffers. Application IDs
filter messages; neither IDs, nonces nor CRC authenticate peers. There are no
legacy codecs or product-specific schemas in this package.

## Validation

```sh
cargo test --locked -p control-link-protocol
python3 tools/check.py build --package control-link-protocol --platform all
python3 tools/check.py clippy --package control-link-protocol --platform all
```

Tests cover fixed wire fields, stream splitting, malformed inputs, buffer limits,
bounded concurrency, ID exhaustion, stale replies, cancellation and discovery
composition. Target builds validate the portable library; runtime/linker and
hardware integration are the responsibility of subsequent adapter validation.

## Runtime adapters

`control-link-tokio` provides TCP (default) and host serial. `control-link-embassy`
provides UART (default), USB CDC serial and TCP. All control links are point to
point. Each adapter retains its decoder and unread bytes from the initial
identity exchange through normal communication; it does not hand off a serial
connection from discovery. The `Buffers` type supplies caller-owned storage.

The adapter halves perform I/O. Compose them with `Requests` and `RequestGate`
for session handling, and supply application identity checks and business logic.
