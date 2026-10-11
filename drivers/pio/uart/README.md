# Async PIO UART wrapper

`pio-uart` wraps Embassy RP 0.10.0's existing async PIO UART TX/RX
types and programs, without copying or replacing their waveforms. It implements
`embedded-io-async` 0.7 `Read` and `Write`, providing short reads and a conservative
physical transmit-completion delay for `flush`. RS-485 direction, turnaround,
packet framing and bus arbitration belong to consumers.

## Construction and resources

Enable exactly one MCU feature: `rp2040` or `rp235xa`. The application supplies
Embassy's time driver and binds its PIO interrupt handler. Board support owns
the clocks, pins, pulls and state-machine assignments.

```rust,ignore
use pio_uart::{PioUart, Programs};

// After creating Embassy's Pio and configuring the board clocks:
let programs = Programs::new(&mut common);
let serial = PioUart::new(
    &mut common, sm0, sm1, tx_pin, rx_pin, &programs, 1_000_000,
)?;
```

One `Programs` loads Embassy's four-instruction TX and ten-instruction RX
programs. Share it across lanes in the same PIO block, rather than loading
duplicate instructions. Each UART owns two distinct state machines and pins,
with eight-word joined TX/RX FIFOs. There are no DMA resources or software RX
buffers. Embassy's RX program uses the relative framing-error IRQ flag
`4 + RX_SM`; reserve it even though Embassy does not expose it through `Read`.
Program memory follows Embassy's allocation lifecycle; construct long-lived
programs/ports once. A dropped wrapper drops its Embassy UART halves and state
machines; it does not control any transceiver's direction pin.

## Baud and completion contract

8N1 is the only format. Embassy configures an integer PIO divider of
`clk_sys / (8 * baud)`. Construction rejects zero/out-of-range rates and any
clock/baud combination that would truncate that divider. For example, 128 MHz
supports exactly 1 Mbaud; the default 125/150 MHz clocks do not. Select a suitable
clock in board support and **keep it unchanged throughout the UART's lifetime**.

Embassy's PIO UART `flush` is a no-op. This wrapper waits for the maximum queued
tail: eight FIFO words plus one OSR character, each ten bits, plus 20 us of
timer margin. The bound is `ceil(90 * 1_000_000 / baud) + 20` microseconds;
at 1 Mbaud this is 110 us. It waits this bound even if the UART is already empty.
This trades turnaround latency for completion
without accessing Embassy's private state machines or inventing a new waveform.
The owned halves are not exposed, preventing another writer from extending the
tail while a flush is pending. Scheduling delays may extend completion.

`write` delegates to Embassy and may queue a prefix before cancellation.
Cancelling does not stop transmission or clear queued bytes. A later successful
`flush` completes the remaining tail; the RS-485 driver uses it with its
transmitter disabled during recovery. `read` returns one byte without waiting
to fill the buffer; an empty read returns immediately. Cancelling a pending
read does not consume a byte.

Receive error reporting retains Embassy's limitations: the trait error is
`Infallible`, framing errors are not surfaced, and FIFO overflow is not reported
to the caller. At 1 Mbaud, eight RX bytes represent about 80 us of buffering.
This wrapper does not establish production receive capacity or four-lane
reliability. Those require hardware qualification and potentially a future
buffering/error-reporting adapter.

## Minimal Embassy example

[`examples/embassy.rs`](examples/embassy.rs) is a standalone Pico/Pico 2
usage example with all setup in one file.

Echoes one byte at a time at 1 Mbaud, 8N1. Connect a 3.3 V USB-UART
adapter TX to GP20 (RX), RX to GP18 (TX), and ground to ground. Send one
byte and wait for its echo before sending the next; the small RX FIFO
is not a sustained-traffic buffer. The example uses PIO1 SM0/SM1 and sets
the system clock to 128 MHz for an exact baud divider.

Build from the workspace root (choose one board):

```sh
# Pico / RP2040
cargo build -p pio-uart --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release --locked
# Pico 2 / RP2350A
cargo build -p pio-uart --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

The ELF is `target/<target>/release/examples/embassy`. Flash the matching
image with your usual SWD/UF2 workflow; an RTT-capable debug probe displays
`defmt` output. These commands build and link only; they do not flash hardware.
The opt-in features supply the executor, time driver, panic logger, boot image,
and package-local Pico/Pico 2 memory layout. Normal library builds keep their
existing runtime and MCU choices. Enable only one example board feature.
Examples stop on I/O errors with `unwrap()` to keep the usage path short;
applications own deadlines and recovery. Builds do not qualify hardware.

## Validation

From the `polis-rs/` root:

```sh
cargo test -p pio-uart
cargo clippy -p pio-uart --all-targets
```

Host tests cover timing bounds and rejected clock/divider configurations.
Electrical timing, cancellation captures and sustained reception remain
hardware-unverified.

## Source and distribution

See [PROVENANCE.md](PROVENANCE.md) for source hashes, extraction changes and
license status.

## License

Licensed under MIT; see [LICENSE-MIT](LICENSE-MIT).
