# Library architecture

Use one virtual Cargo workspace with independently consumable and independently
versioned library packages. Separate portable contracts, hardware drivers, and
platform integration. Share dependency declarations without requiring consumers
to adopt the whole workspace.

## Package boundaries

| Location | Responsibility | Packages |
| --- | --- | --- |
| `crates/` (group related packages in subdirectories) | Codecs, shared protocol definitions, reusable algorithms and host libraries | `polis-framing`, `polis-discovery-protocol`, `polis-control-link-protocol`, `polis-led-output` |
| `drivers/` and `drivers/pio/` | Peripheral behavior, bus timing, electrical direction, device recovery | `polis-rs485`, `polis-tmp1x2`, `polis-pio-uart` |
| Package `examples/` and `tests/` | Small usage demonstrations, mocks, compatibility and link checks | TCP/UDP framing demos; explicit MCU link examples |
| Product repositories | Composition and deployment | Board pins, product messages, boot images, service entry points |

Create a package when it has a coherent public responsibility and a real consumer.
Avoid a catch-all `common` crate, an umbrella dependency that brings every driver,
and placeholder packages. Host-only libraries are welcome; they need not pretend
to support `no_std`.

Every workspace library uses `polis-<purpose>` as its Cargo package name, with
hyphens between words. PIO drivers use `polis-pio-<device>`; runtime adapters use
`polis-<protocol>-<runtime>`, such as `polis-discovery-tokio`. Add an adapter only
when actual runtime integration warrants a separate crate.

Rust crate names replace hyphens with underscores (`polis_framing`,
`polis_pio_uart`). Use the package name as the dependency key by default;
intentional dependency aliases remain supported. Directory names stay short and
do not need to match the package prefix. Check crates.io name availability when
adding a package and again before publication; the prefix does not reserve names.

## Dependency direction

Portable algorithms and wire contracts sit below platform adapters. Drivers
accept resources through `embedded-hal`, `embedded-hal-async`, or
`embedded-io-async` where those traits fit. RP PIO drivers may use Embassy RP
directly. A W5500 adapter tied to Embassy should document that runtime dependency
even if its SPI/GPIO interface uses generic traits.

Product firmware composes those libraries with its BSP and executor. Host
applications compose them with their own sockets/runtime. Libraries do not choose
product pins, global allocators, panic handlers, executors, network interfaces,
or linker layouts. Shared host parsers remain outside embedded runtime dependency
graphs; build-time use is a separate dependency edge.

Keep framing independent of discovery and product messages. Discovery
adapters compose payload definitions, framing, I/O and a clock. Discovery
payload definitions must remain usable without a socket, driver or executor.
Extract state machines separately from platform I/O only when real code exists.

## Shared control link

`crates/control-link/control-link-protocol` owns the control envelope and bounded
request/session tracking. It depends on `polis-framing` in `crates/protocol-framing`;
it has no runtime dependency on discovery. Request and response bodies remain
application-owned, as do business handlers and handshake identity checks.
The shared envelope replaces downstream transport envelopes during adoption.

`Requests<N>` matches replies while allowing unsolicited events and concurrent
keepalive requests. `RequestGate` rejects repeated/decreasing IDs before business
invocation. Runtime adapters own framed I/O and retain buffers through startup, the session
handshake and ongoing communication. Applications compose the trackers with
identity checks, handlers, deadlines and any queues/reconnect policy they need;
operations with uncertain outcomes are never automatically replayed. Control-link
is always point-to-point, including UART/USB serial.

Related packages live under `crates/discovery/` and `crates/control-link/`.
Their `-protocol` packages are portable. Discovery's `-tokio` and `-embassy`
adapters expose independently optional `udp` and `rs485` features. Tokio defaults
to UDP; Embassy selects no transport by default. Control-link runtime
adapters default to TCP on Tokio and UART on Embassy. Tokio also supports host
serial; Embassy also supports USB CDC serial and TCP over any network interface. Hardware resources and
network configuration stay downstream.

## Shared discovery protocol

`crates/discovery/discovery-protocol` owns the discovery protocol used across products,
including its wire schema, message IDs, device identity and discovery agreements.
It is a discovery library, not a collection of unrelated product protocols.

The consumer supplies an application identifier when constructing discovery
configuration. Its fixed 16-byte identifier is carried on the wire and validated on receive so
an application can discover its own devices using the common protocol. Do not
add an enum of known products to the shared crate. Consumers own identifier
assignment, deployment endpoint overrides and any product-specific metadata.
Application identity is routing/filtering information, not authentication.

The common schema and cross-consumer interoperability tests live with the crate.
Migrating existing products to it is a later consumer update; the old byte layouts
are not automatically changed by adding the shared library.

UDP discovers network endpoints; RS-485 discovers participants sharing a bus.
Point-to-point UART/USB serial discovery is intentionally absent. Its connection
and session lifecycle belongs to control-link, without a discovery/control handoff.

RS-485 adapters execute controller rounds and participant slots using portable
state from polis-discovery-protocol and caller-supplied timing bounds. Their framed I/O
helpers are private. The configured backend owns electrical direction and physical
TX completion; hardware UART can be part of that backend. Participant callbacks
still expose other protocols' borrowed frames for application dispatch.

UDP adapters send/receive one frame per datagram and offer announce, listen,
probe/respond and combined serve behaviors. These roles are independent of
application server/client labels and of who later connects or accepts TCP.
Applications supply sockets, interfaces, identities and registry handling.
Returned source endpoints identify the discovery route; the application supplies
the control service's endpoint/port.

## Embedded and host features

Portable embedded libraries default to allocation-free `no_std`. Introduce
additive `alloc`, `std`, `serde`, or `defmt` features only where they expose a
useful supported configuration. Retain framing's current serialization dependency
in the first extraction; optionalizing it is a separate API change.

HAL drivers default to no selected MCU. Build `rp2040` and `rp235xa` in separate
Cargo invocations, reject conflicting selections clearly, and document what
the no-feature API exposes. Do not enable a chip, executor or time driver in
workspace-wide dependencies. Optional logging also belongs to the package or
consumer that requests it. Cargo features unify, so neither resolver 3 nor
`--all-features` isolates incompatible MCU configurations.
[Cargo feature behavior](https://doc.rust-lang.org/cargo/reference/features.html)
explains why these builds must stay separate.

Group PIO libraries under `drivers/pio/{i2c,uart,sk6812,ws2813}`. Shared workspace
dependencies pin HAL versions; a repository build command's `--platform` option
selects the target and forwards the corresponding package features. A virtual
workspace has no package features of its own. Keep the public package features
usable directly by downstream Cargo consumers; avoid custom global cfg flags
that silently change dependency selection.

## Product consumption

Before registry release, products use a reviewed Git revision from the future
repository remote. After release, use a normal versioned registry dependency.
These are templates, not usable remote coordinates today:

```toml
# Before publication: replace both placeholders with real values.
[dependencies]
polis-framing = { git = "<polis-rs repository URL>", rev = "<full commit SHA>" }
```

```toml
# After polis-framing 0.1.0 is published:
[dependencies]
polis-framing = "0.1.0"
```

For an initial migration, dependency aliases can preserve imports:

```toml
legacy-framing = { package = "polis-framing", git = "<polis-rs repository URL>", rev = "<full commit SHA>" }
```

Replace the old dependency declaration and remove the old crate's workspace
membership during cutover. Every dependency edge must resolve to the same
upstream crate; retaining an old path copy can produce distinct Rust types.

For local development, a product can temporarily override a registry dependency
in its root manifest with `[patch.crates-io]` and a local package path. A Git
dependency requires a patch for that Git source URL instead. The local package
version must satisfy the consumer's version requirement. Do not commit paths
specific to a developer's checkout. [Cargo dependency sources](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html)
describe the source-selection rules.

Products retain their own lockfiles, compiler pins, profiles and target settings.
Our workspace lockfile/toolchain/profile does not impose those choices on them.
Update products deliberately after upstream validation; a source of truth does
not require products to track a moving Git branch.
