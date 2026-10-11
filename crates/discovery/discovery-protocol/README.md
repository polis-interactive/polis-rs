# Shared discovery protocol

`discovery-protocol` defines one allocation-free `no_std` discovery protocol for
embedded and host consumers. Configure it with your application's 16-byte
`ApplicationId`; use those same bytes in every peer. The library contains no
product enum, sockets, clock or executor. Portable role/session state stays here;
runtime adapters execute its I/O and deadlines.

```rust
use discovery_protocol::{ApplicationId, Discovery, Discover, Message,
    MAX_PAYLOAD_LEN, PROTOCOL_ID, SCHEMA_VERSION};

// The application owns and persists this assignment. It is not a secret.
const APP: ApplicationId = ApplicationId(*b"example-app-0001");
let controller = Discovery::new(APP);
let device = Discovery::new(APP);
let request = Message::Discover(Discover { discovery_id: 42 });
let mut scratch = [0; MAX_PAYLOAD_LEN as usize];
let payload = controller.encode_payload(&request, &mut scratch)?;
let decoded = device.decode_payload(PROTOCOL_ID, SCHEMA_VERSION,
    request.message_type(), payload)?;
assert_eq!(decoded, request);
# Ok::<(), discovery_protocol::Error>(())
```

Peers with the same application identifier use the same message format across
UDP and framed RS-485 buses. A different application returns `WrongApplication`
before body decoding. Consumers coordinate unique application assignments; the
library neither allocates them nor treats any value as a wildcard. Application
identity filters discovery and does not authenticate a sender.

## Schema 1

The framing header uses protocol ID `0x5044` (Polis Discovery), schema `1`, and
the explicit message ID below. These are locally defined protocol assignments.
Every payload starts with exactly 16 raw application-ID bytes, without a length
or enum tag. The remaining body uses Postcard: unsigned LEB128 integers,
declaration-order fields, and eight raw bytes for `DeviceId`. There is no
in-memory `Message` enum discriminant on the wire.

| Message ID | Body in wire order | Maximum total payload bytes |
| --- | --- | --- |
| `0x0001` Discover | `discovery_id: u64` | 26 |
| `0x0002` Announcement | `discovery_id: u64`, `device_id: [u8; 8]` | 34 |
| `0x0101` RS-485 round | `discovery_id: u64`, `seed: u32`, `slot_count: NonZeroU16`, `slot_duration_us: NonZeroU32`, `turnaround_us: u32` | 44 |
| `0x0102` RS-485 acknowledgment | `discovery_id: u64`, `device_id: [u8; 8]` | 34 |

`MAX_PAYLOAD_LEN` is 44, including the application prefix. Requests, rounds and
acknowledgments require nonzero discovery IDs. Announcement ID zero is reserved
for unsolicited announcements on UDP. Unknown protocols, schemas and message IDs,
invalid/truncated bodies, and trailing bytes are rejected. Schema negotiation
is not implemented. Schema changes require explicit versioning and fixtures.

Compose this codec with `framing`: set `Header` from `PROTOCOL_ID`,
`SCHEMA_VERSION`, and `message.message_type()`, then frame the bytes returned
by `Discovery::encode_payload`. On receive, validate framing first and pass its
header fields and payload to the configured `Discovery::decode_payload`.
Use `wire_capacity(MAX_PAYLOAD_LEN)` for framing buffers. UDP carries exactly one
complete frame per datagram; stream adapters retain their incremental decoder.
Do not transmit output after an encoding error.

This defines a new protocol family; compatibility with earlier wire schemas is
not implied. Product control, OTA and runtime messages are outside its scope.

## UDP and identity

The shared default multicast endpoint is `239.80.68.1:43000`. Consumers may
override it for their deployment; peers still need a common reachable endpoint.
The address is a deployment default, not a registered service assignment.
Interface selection, announcements, retries and jitter belong to I/O adapters.

`DeviceId` is a provisioned stable eight-byte identity across reboots and links.
All byte patterns are ordinary IDs. Provisioning and duplicate detection belong
to the consumer. A controller chooses nonzero discovery IDs and avoids reuse
while delayed packets or retained sessions can exist, including across reboots.

## RS-485 agreement

Exactly one controller schedules a physical bus, even if multiple application
IDs share it. Application filtering does not permit independent controllers to
transmit concurrently. A bare Discover grants no transmission rights; only a
matching application's DiscoveryRound grants response slots.

Unacknowledged devices choose `round.slot_for(device_id)`. Slot selection uses
FNV-1a/32 over seed little-endian bytes followed by device-ID bytes, then MurmurHash3
fmix32, modulo slot count. Fixed test vectors define the interoperable result.
FNV offset/prime are `0x811c9dc5`/`0x01000193`; fmix uses shifts 16/13/16 and
multipliers `0x85ebca6b`/`0xc2b2ae35`, all with wrapping u32 arithmetic.

Timing is relative to the request's final stop bit. Slot i starts at
`turnaround_us + i * slot_duration_us`; the complete announcement, driver release
and margin must fit. Reject schedules the physical link cannot meet. The
controller stays silent through the full response window, then acknowledges each
observed device. Retry rounds retain the session ID and change the seed.

A device stops replying only after its own acknowledgment in the current
application/session. Lost acknowledgments require duplicates to be acknowledged
again. Reboots and new sessions make devices eligible again. Collisions can look
like silence, corruption or one decodable reply; a quiet round does not prove
completeness.

`Participant` implements ACK suppression, consecutive duplicate-seed suppression,
and a conservative reply window. `Timing` supplies a receive timestamp error
bound and a full transmission/bus-release budget; a reply must fit even if the
observation timestamp is early or late by that bound. `Collection<N>` retains a
bounded set of identities for ACKs after the response window. It reports overflow
without acknowledging identities it could not retain. These types allocate nothing
and own no clock or I/O. Runtime adapters enforce deadlines and bus phases;
applications select retries, session/seed generation and address assignment.

For UDP, `reply_to` constructs the correlated response to a
Discover and `Announcement::unsolicited` constructs a startup/periodic announcement.
`reply_to` deliberately ignores RS-485 rounds; they require scheduled responses.

## Validation and distribution

Run `cargo test -p discovery-protocol` for independent byte fixtures, malformed-input
and application-filter tests, maximum sizes, slot vectors and stream/datagram
composition. `python3 tools/check.py build --package discovery-protocol --platform all`
builds it on host, RP2040 and RP2350. See [PROVENANCE.md](PROVENANCE.md) for its
origin and changes. This package is licensed under MIT; see LICENSE-MIT.
