# PIO I²C controller

`pio-i2c` supplies the physical I²C bus for RP2040 and RP2350A,
with an API shaped like Embassy RP 0.10.0's PIO SPI driver. It is adapted from Raspberry Pi's
[`i2c.pio`](https://github.com/raspberrypi/pico-examples/blob/master/pio/i2c/i2c.pio)
and [`pio_i2c.c`](https://github.com/raspberrypi/pico-examples/blob/master/pio/i2c/pio_i2c.c).
Upstream attribution is retained under [BSD-3-Clause](LICENSE-BSD-3-Clause).

## API and resource ownership

Enable exactly one MCU feature: `rp2040` or `rp235xa`. The application must
provide an Embassy time driver for both blocking deadlines and async timeouts. BSP supplies the PIO
`Common`, one state machine, SCL and SDA (in that order), and external pull-ups.
Create `Pio` with Embassy's `InterruptHandler` binding before construction.

```rust,ignore
use pio_i2c::{Config, I2c};

let mut config = Config::default();
config.frequency = 400_000;
let mut bus = I2c::new(&mut common, sm0, scl, sda, config);
let mut temperature = [0; 2];
bus.write_read(0x48, &[0x00], &mut temperature).await?;
```

`I2c<'d, PIO, SM, Async>` has `new`, `read`, `write`, `write_read`, `transaction`
and `recover`. `I2c<..., Blocking>` has `new_blocking`. Both modes expose
`blocking_read`, `blocking_write`, `blocking_write_read`, `blocking_transaction`
and `blocking_recover`. Both implement `embedded-hal` 1.0 I²C; async mode also
implements `embedded-hal-async` 1.0 I²C. `set_frequency` and `set_config` return
`ConfigError` on invalid settings, preserving the previous configuration.

The driver owns its loaded program, pins and state machine, like Embassy's PIO
SPI abstraction. Each instance allocates **20 instructions**. No DMA channels or
PIO IRQ flags are required: RX FIFO interrupts signal completed bytes/commands.
Dropping releases the lines and disables the machine; its program allocation
remains reserved in `Common`. Construct long-lived buses once. Pins belong to BSP;
no board IDs, sensor addresses or sensor register formats live in the driver.

## Pin order and timing

SDA and SCL may be any two distinct RP2040/RP2350A GPIOs, including nonadjacent pins.
Every SCL `WAIT PIN` uses `(scl - sda) mod 32`, with SDA as `IN_BASE`:

| SDA | SCL | WAIT offset |
| --- | --- | --- |
| GP6 | GP5 | 31 |
| GP6 | GP7 | 1 |

On RP2350A, construction uses `unstable-pac` to set the owned state machine's
`IN_COUNT` to 0 (all 32 inputs) after HAL configuration. Otherwise Embassy's
single SDA input masks SCL from `WAIT PIN`, including reversed offset 31.
This does not claim or change any additional GPIOs. See the RP2350 datasheet
[PIO register description](https://datasheets.raspberrypi.com/rp2350/rp2350-datasheet.pdf).

One driver in PIO0 SM0 plus the shared four-instruction SK6812 program in SM1/SM2
uses **24 of 32 instructions**.

Output latches always stay low. PIO changes only output enables: drive low or
release to the pull-ups. SDA payload bits are inverted in software, avoiding the
upstream GPIO OE inversion requirement. Read data and ACK bits are sampled from
the actual SDA input. Both pins retain Schmitt inputs and input synchronization.
Internal pull-ups default off; size external pull-ups for the bus capacitance.

Configuration defaults to 100 kHz and a 100 ms **whole-transaction** timeout.
Supported frequencies are up to 400 kHz, subject to the divider range;
`clk_sys >= 32 MHz` is required and must stay fixed for the driver's lifetime.
The divider rounds upward. A data bit takes 36 PIO cycles: nominally 20 low and
16 high. After clock stretching ends, at least 15 high cycles remain. This
preserves Standard/Fast-mode low/high minima even with divider quantization;
START/STOP phases include explicit setup/hold and bus-free delays. FIFO/executor
delays between bytes can reduce throughput below the configured SCL rate.
Timing requirements come from [NXP UM10204](https://www.nxp.com/docs/en/user-guide/UM10204.pdf).

## Transactions, errors and cancellation

Seven-bit addressing only; no multi-controller arbitration, target/slave mode,
10-bit addressing, SMBus PEC or Fast-mode Plus. Reserved 7-bit addresses are not
filtered by this driver; address policy belongs to the caller.
An empty write probes the address. Empty reads are rejected before bus activity;
an empty operation list does nothing. Consecutive operations in the same
direction concatenate without extra START/address cycles. Direction changes use
repeated START. Only the last byte of a contiguous read is NACKed by the controller.

Unlike upstream's example, **all write NACKs, including the final byte**, are
errors. Address and data NACKs are distinguished. Each byte returns a nine-bit
sample only after the ACK clock falls. Command packets return an RX token after
their instructions finish, so successful methods return after STOP/bus-free time,
not merely after filling a FIFO. NACKs attempt STOP before returning the error.

Clock stretching is honored during bytes, START/repeated START, STOP and recovery.
A transaction timeout or a dropped, polled async future disables/restarts the
machine, discards FIFOs and releases both lines. This cannot undo partial writes
or guarantee a clean target state. The next operation returns `RecoveryRequired`.
Call `recover`/`blocking_recover`: nine clock pulses with SDA released, then STOP.
If either line remains held low, recovery times out and the bus stays marked for
recovery. Only use this bus-clear operation on a single-controller bus. After
ordinary NACK plus successful STOP, the bus is immediately reusable.

## Minimal Embassy example

[`examples/embassy.rs`](examples/embassy.rs) is a standalone Pico/Pico 2
usage example with all setup in one file.

Reads the TMP102/TMP112 configuration register once per second through
the standard async I²C trait and logs its two bytes over RTT. It uses PIO0
SM0 with SDA on GP6 and SCL on GP7, external pull-ups to 3.3 V, sensor
ADD0 grounded (0x48), a 3.3 V supply, and common ground. This demonstrates
`write_read` without adding a sensor-driver dependency.

Build from the workspace root (choose one board):

```sh
# Pico / RP2040
cargo build -p pio-i2c --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release --locked
# Pico 2 / RP2350A
cargo build -p pio-i2c --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
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

```sh
cargo test -p pio-i2c
cargo build -p pio-i2c --features rp2040 --target thumbv6m-none-eabi --release
cargo build -p pio-i2c --features rp235xa --target thumbv8m.main-none-eabihf --release
```

Host tests interpret the emitted PIO instructions with the configured FIFO/shift
settings. They cover bit order, open-drain control, ninth-bit completion, normal
and reversed/nonadjacent SCL offsets, pulse timing under stretching, transaction
boundaries, adjacent operations, NACKs, address probes and bus-clear packets.

These checks do not qualify electrical behavior. Measure both pin orders at
100/400 kHz, START/repeated START/STOP, pull-up rise times, NACK recovery,
cancellation, held-low lines, clock stretching and coexistence with LED output
on hardware before qualifying a bus.

## Source and distribution

See [PROVENANCE.md](PROVENANCE.md) for source hashes, extraction changes and
license status.
