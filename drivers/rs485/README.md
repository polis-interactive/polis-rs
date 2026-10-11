# Async RS-485

`rs485` is a portable, allocation-free `no_std` half-duplex driver.
It consumes `embedded_io_async::Read` / `Write` (0.7), an infallible
`embedded_hal::digital::OutputPin`, and `embedded_hal_async::delay::DelayNs`.
It has **no Embassy, PIO, MCU, executor or clock dependency**.

## API and ownership

```rust,ignore
use embedded_hal::digital::PinState;
use embedded_io_async::Read;
use rs485::{Config, Rs485};

let mut bus = Rs485::new(serial, direction, delay, Config {
    tx_level: PinState::High,
    enable_us: 12,
    hold_us: 1,
    receive_us: 12,
});
bus.recover().await?;
bus.send(&request).await?;
let count = bus.read(&mut reply).await?;
```

The sample intervals are conservative THVD1420 bench values, not defaults for
arbitrary transceivers. Board support supplies polarity, timing, safe pin
initialization and the serial configuration. This driver supports tied DE and
active-low RE on a single direction output, as in Slate's hardware contract.
The opposite of `tx_level` selects receive. A separate-enable topology needs
its own direction abstraction or adapter.

`new` immediately selects receive. `recover` flushes pre-existing UART data with
transmission disabled and waits for receive settling before allowing traffic.
Initialization should keep DE inactive before constructing the driver; a
hardware pull resistor must preserve that state during reset and pin release.

`send` holds direction across all short writes of one slice, calls serial
`flush`, waits `hold_us`, releases direction, and waits `receive_us`. It returns
only after these steps. It also implements `Write`: each successful `write`
sends the entire slice; separate calls have separate direction assertions.
`Read` delegates to the serial reader in receive mode. Empty reads/sends return
immediately without touching the pins or waiting, even when recovery is needed.

Caller code owns packet boundaries, addresses, CRCs, bus arbitration,
reply deadlines and retries. Exclusive mutable access serializes this driver's
operations. Receive buffering and overflow/error reporting belong to the serial
backend; this crate does not add a software ring or discard buffered replies.

## Flush and cancellation contract

The serial backend's `Write::flush` **must wait until the final stop bit has
finished on the wire**. Merely accepting bytes or emptying a software/hardware
FIFO is insufficient. The driver cannot infer this from trait conformance.
Embassy RP 0.10.0's stock PIO UART is async, but its `flush` is a no-op and needs
an adapter before use here. Hardware UART backends also need their actual flush
semantics checked. The separate [PIO UART wrapper](../pio/uart/README.md)
provides that completion guarantee around Embassy's existing PIO programs. The
bench (in the upstream Slate project) composes the two drivers; the UART wrapper
is not a dependency of this crate.

Dropping a pending send future (including caller timeouts), or returning a write
or flush error, immediately releases DE through a drop guard. It leaves the
driver in `RecoveryRequired`; the UART may still contain queued transmit data.
`embedded-io-async` provides no abort operation. `recover` therefore drains those
bytes with DE inactive, intentionally discarding the suffix at the RS-485 link.
It must complete before another nonempty send/read. A cancelled or failed
recovery leaves the driver blocked. Even cancellation during the final receive
settling delay conservatively requires recovery.

This is bus-release safety, not delivery atomicity. The peer may have received
a partial packet and the protocol must resynchronize. `recover` does not purge
RX or acknowledge/disguise that loss. `flush` on the RS-485 wrapper reports
`RecoveryRequired` instead of silently recovering. Read cancellation and errors
inherit the underlying reader's semantics; `read_exact` can consume a prefix.
`recovery_required()` lets callers distinguish TX recovery from RX/protocol work.

The UART must remain usable after cancelling its I/O future; if it needs an
additional hardware reset, that belongs in its adapter. Apply deadlines outside
the driver to bound sends, reads and recovery. Configuration delays are minimum
waits; scheduling latency can extend bus ownership. Other nodes must honor the
protocol's turnaround budget. Dropping the driver selects receive; board biasing
must also cover the direction pin's subsequent destructor/reset behavior.

## Validation

From the `polis-rs/` root:

```sh
cargo test -p rs485
cargo clippy -p rs485 --all-targets
```

The image builds also compile this portable crate on both embedded targets.
Host tests exercise ordering, both polarities, short writes, error propagation,
every send cancellation point, recovery failures/cancellation and stale TX
suppression. They use standard-trait mocks with no MCU dependency. Product BSP
integration, four-lane concurrency, receive capacity, stop-bit/DE timing and
electrical behavior require separate hardware qualification.

## Source and distribution

See [PROVENANCE.md](PROVENANCE.md) for source hashes, extraction changes and
license status. Product BSPs and bench firmware remain in their original projects.

## License

Licensed under MIT; see [LICENSE-MIT](LICENSE-MIT).
