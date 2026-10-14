# WS2813 PIO driver

`polis-pio-ws2813` drives `WS2813B-B` RGB chains on RP2040 or RP2350A. It adapts
Embassy RP 0.10.0's `PioWs2812`, with the drain/reset and output initialization
approach used by this workspace's SK6812 driver. Upstream attribution is
retained under [MIT](LICENSE-MIT).

## API and resources

Enable exactly one of `rp2040` or `rp235xa`. The application supplies an Embassy
time driver; board support supplies the pin, state machine, DMA channel and
DMA interrupt binding. Load `PioWs2813Program` once per PIO block and keep it
loaded while its chains exist. It occupies four instructions shared by all
chains on that block. RP2350A supports PIO0, PIO1 and PIO2.

`PioWs2813<PIO, SM, N>::new(...)` constructs one fixed-length chain.
`write(&[RGB8; N]).await` sends exactly `N` pixels in 24-bit GRB order,
most-significant bit first. Each chain owns one output pin, one state machine
and one DMA channel. Packing uses an inline `4 * N` byte buffer in the write
future, with only the upper 24 bits of each word transmitted. There is no heap
allocation. Separate chain writes can run concurrently.

`N` must be nonzero. `clk_sys` must be at least 32 MHz and remain unchanged
while the driver exists. GPIO ownership and peripheral availability belong
to the BSP. The driver implements `polis_led_output::LedOutput<RGB8>` for fixed-layout
concurrent RGB output groups. Slice writes reject wrong lengths before I/O;
no RGB/RGBW color conversion is performed.

## Data signaling

PIO controls pulse timing; the GPIO supply and any external buffer determine
voltage. Choose signal levels and wiring to meet the LED datasheet requirements.
Connect controller and LED supply grounds together.

## Timing and completion

The timing contract follows the **WS2813B-B V4**
[Worldsemi datasheet, pages 3–4](https://www.tme.eu/Document/8523e928e69088ecbec7f9569ae0f9a8/WS2813B-B%20%28Ver.%20No.V4%29_EN.pdf).
Other WS2813 revisions are not implicitly qualified.

At nominal 800 kbit/s, each bit uses 20 PIO cycles:

| Interval | Nominal output | Datasheet bounds |
| --- | --- | --- |
| Zero high | 312.5 ns | 220–380 ns |
| Zero low | 937.5 ns | 580–1000 ns |
| One high | 625 ns | 580–1000 ns |
| One low | 625 ns | 580–1000 ns |
| Reset low | At least 350 µs | More than 280 µs |

The divider rounds upward so the average bit rate does not exceed 800 kbit/s.
Tests include fractional-divider jitter, the 32 MHz lower bound, and the
125/150 MHz Pico/Pico 2 defaults. The program stalls low when awaiting data.

DMA completion alone does not mean the last pixel has left PIO. The driver
waits a conservative bound for eight FIFO words, the shift register and an
in-flight bit, followed by 350 µs reset. The combined wait is 622 µs at both
default clocks. Initial writes establish the same drain/reset interval before
transmitting; subsequent completed writes need only the final wait.

Cancellation aborts DMA but may display a partial frame. The next write waits
for remaining PIO data to drain and reset before starting a full frame. Dropping
the driver disables its state machine; board support must initialize the output
low before reuse. Construction preloads the owned PIO output low before pin mux
handover and configures a pull-down, using Embassy's `unstable-pac` feature.

Applications must supply DMA bandwidth without mid-frame starvation; the driver
cannot detect it. Concurrent completion does not guarantee simultaneous optical
latching on unequal chains. Backup-data wiring remains a board responsibility,
not a second independently generated LED stream.

## Minimal Embassy example

[`examples/embassy.rs`](examples/embassy.rs) is a standalone Pico/Pico 2
usage example with all setup in one file.

Blinks eight WS2813 RGB LEDs dim red every 500 ms using PIO0 SM0 and
DMA_CH0. Connect GP2 through a suitable level shifter to chain DIN;
wire backup data according to the strip/part requirements, use an
appropriately sized LED supply, and connect common ground. Set
`LED_COUNT` to your chain length. The existing `link_check` example
remains a separate concurrent-chain integration fixture.

Build from the workspace root (choose one board):

```sh
# Pico / RP2040
cargo build -p polis-pio-ws2813 --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release --locked
# Pico 2 / RP2350A
cargo build -p polis-pio-ws2813 --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

The ELF is `.target/<target>/release/examples/embassy`. Flash the matching
image with your usual SWD/UF2 workflow; an RTT-capable debug probe displays
`defmt` output. These commands build and link only; they do not flash hardware.
The opt-in features supply the executor, time driver, panic logger, boot image,
and package-local Pico/Pico 2 memory layout. Normal library builds keep their
existing runtime and MCU choices. Enable only one example board feature.
Examples stop on I/O errors with `unwrap()` to keep the usage path short;
applications own deadlines and recovery. Builds do not qualify hardware.

## Validation

Run from the workspace root:

```sh
cargo test -p polis-pio-ws2813
cargo clippy -p polis-pio-ws2813 --all-targets -- -D warnings
cargo build -p polis-pio-ws2813 --features rp2040 --target thumbv6m-none-eabi --release
cargo build -p polis-pio-ws2813 --features rp235xa --target thumbv8m.main-none-eabihf --release
cargo build -p polis-pio-ws2813 --example link_check --features rp2040,link-check --target thumbv6m-none-eabi --release
cargo build -p polis-pio-ws2813 --example link_check --features rp235xa,link-check --target thumbv8m.main-none-eabihf --release
```

For target Clippy, replace `build` in each example command with `clippy` and
append `-- -D warnings`. The opt-in example supplies its own Pico/Pico 2 linker
layout; ordinary library builds impose no image layout. It links concurrent
63- and 70-pixel chains on GPIO2/GPIO3, using PIO0 on RP2040 and PIO2 on RP2350A,
and repeatedly sends black. It validates linking, not visual output.

Host tests interpret the emitted PIO instructions, check GRB/MSB-first ordering
and exclusion of padding, and verify pulse and drain/reset bounds. Linked
examples establish type/resource integration, not electrical behavior.
Hardware qualification remains pending: measure pulse widths, reset and startup
at the LED inputs; exercise colors, concurrent DMA traffic, cancellation and
restart on the target hardware.

## Source and distribution

See [PROVENANCE.md](PROVENANCE.md) for source hashes, extraction changes and
license status.
