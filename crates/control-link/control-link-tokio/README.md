# polis-control-link-tokio

Point-to-point, bidirectional control communication. Keep one `Sender` and
`Receiver` from connection setup through the identity handshake and normal
traffic. Either peer can originate requests, replies, and events.

| Feature | Transport | Default |
| --- | --- | --- |
| `tcp` | Connected or accepted Tokio TCP socket | Yes |
| `serial` | Host serial port, including USB CDC | No |

`stream` is always available for other Tokio `AsyncRead`/`AsyncWrite` backends.
Disable default features for a serial-only dependency. There is no discovery or
RS-485 dependency. Wi-Fi uses the TCP adapter through the host network stack.

## One connection, independent directions

`tcp::split` accepts a socket returned by either `TcpStream::connect` or
`TcpListener::accept`. It enables TCP_NODELAY and returns `(sender, receiver)`.
`serial::open` takes a caller-configured `tokio_serial::SerialPortBuilder`;
`serial::split` accepts an already-open port. Port selection and settings stay
with the application.

```rust,no_run
#[cfg(feature = "tcp")]
async fn example(socket: tokio::net::TcpStream) -> Result<(), Box<dyn std::error::Error>> {
    use polis_control_link_tokio::{
        Buffers,
        protocol::{ControlLink, Incoming, Message, Requests},
        tcp,
    };
    use std::num::NonZeroU64;

    let link = ControlLink::new(*b"example-app-0001");
    let (mut receive, mut payload, mut transmit) = ([0; 320], [0; 284], [0; 320]);
    let (mut sender, mut receiver) = tcp::split(
        socket,
        link,
        Buffers {
            receive: &mut receive,
            payload: &mut payload,
            transmit: &mut transmit,
        },
    )?;
    // Illustrative nonce only: generate a fresh session for each real connection.
    let mut requests = Requests::<4>::new(NonZeroU64::new(1).unwrap());
    let hello = requests.reserve()?;
    sender
        .send(Message::Request {
            key: hello,
            body: b"identify",
        })
        .await?;
    loop {
        let identity = receiver
            .receive(
                |message| -> Result<_, polis_control_link_tokio::protocol::StateError> {
                    match requests.receive(message)? {
                        Incoming::Response { key, body } if key == hello => Ok(Some(body.to_vec())),
                        Incoming::Event { body } => {
                            /* dispatch the event */
                            let _ = body;
                            Ok(None)
                        }
                        _ => Ok(None),
                    }
                },
            )
            .await??;
        if let Some(identity) = identity {
            // Validate the application-owned identity; keep using these same halves.
            let _ = identity;
            break;
        }
    }
    Ok(())
}
```

`receive` returns one decoded message. Its synchronous callback borrows the body;
deserialize/copy it into an owned value for an async handler. Unread messages and
partial frames stay in the receiver. Keep one receive loop per connection. The
separate sender can be used while receiving, including during a long operation.
Queues, spawned tasks, timeouts and handler concurrency are application choices.

## Session and failure handling

The adapter performs framed I/O; it does not approve identities, allocate request
IDs or silently filter stale sessions. Compose it with `protocol::Requests<N>`
for pending outgoing requests and `protocol::RequestGate` before invoking an
incoming request handler. Route incoming requests separately from replies/events.
Application payloads define the handshake: an initial ordinary request/response
can exchange identity using the same stream. Retain the gate and tracker through
that handshake. Session IDs and application IDs provide routing, not authentication.

A receive may be cancelled safely on Tokio; its buffered bytes remain available.
A cancelled or failed **send** can have transmitted part or all of a command.
The sender then rejects further sends with `Error::Interrupted`: retire both
halves, establish a fresh session, and report an uncertain outcome. There is no
automatic reconnect or request replay. Codec failures occur before I/O and leave
the sender usable. A complete send is not a command acknowledgement.

EOF or an I/O error retires the receiver. EOF with an incomplete frame reports
truncation once. Framing/protocol errors are surfaced individually; the caller
can close the connection or continue decoding. A timeout does not cancel work
at the peer; use `Requests::cancel` to retire the local slot deliberately.

For a maximum body size B, allocate `PREFIX_LEN + B` payload bytes and
`polis_framing::wire_capacity((PREFIX_LEN + B) as u16)` bytes for each of receive and
transmit, with B <= `MAX_BODY_LEN`. Buffers are caller-owned. `into_inner` recovers
transport resources when retiring a link; using it for a handshake handoff would
discard buffered input.
