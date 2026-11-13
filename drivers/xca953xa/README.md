# Async XCA953x GPIO expanders

`polis-xca953xa` provides an original, allocation-free `no_std` async I²C driver.
`Xca9534` supports these eight-bit parts at the register-protocol level:

| Parts | Seven-bit addresses | Address-pin constructor |
| --- | --- | --- |
| PCA9534 / TCA9534 | 0x20–0x27 | `Address::from_pins(Variant::Standard, a2, a1, a0)` |
| PCA9534A / TCA9534A | 0x38–0x3f | `Address::from_pins(Variant::A, a2, a1, a0)` |

These parts share input (0), output-latch (1), polarity (2) and configuration (3)
registers. They differ in electrical specifications; consult the fitted part's
datasheet. The family package name does not claim support for other XCA953x
parts, including four-bit expanders or sixteen-bit parts with different maps.
There is no device-ID register and no automatic part detection.

The library uses `embedded-hal-async` 1.0 and selects no MCU, allocator, timer or
executor. It accepts an owned/borrowed bus or an Embassy shared-bus proxy. The
implementation follows TI's [TCA9534 datasheet, SCPS197D, sections 8.3–8.6](https://www.ti.com/lit/ds/symlink/tca9534.pdf).
Additional datasheets and reference-library revisions are in [PROVENANCE.md](PROVENANCE.md).

## Usage

```rust
use embedded_hal_async::i2c::I2c;
use polis_xca953xa::{Address, Config, Direction, Level, Pin, Xca9534};

async fn use_port<B: I2c>(bus: B) -> Result<(), B::Error> {
    let mut port = Xca9534::new(bus, Address::DEFAULT); // 0x20; no I/O
    port.init(Config { outputs: 0, polarity: 0, inputs: 0xfe }).await?;
    let sample = port.read_pin(Pin::P1).await?;
    port.set_level(Pin::P0, sample).await?;

    // Load the latch before enabling another output.
    port.set_level(Pin::P2, Level::Low).await?;
    port.set_direction(Pin::P2, Direction::Output).await?;
    Ok(())
}
```

`Address::new` rejects values outside the two supported ranges, including shifted
eight-bit addresses. `Pin::new` validates 0–7. `Address::DEFAULT` is 0x20 and
`Address::A_DEFAULT` is 0x38. Strap constructors take A2, A1, A0 in that order.

`read_inputs` samples all physical pins using the input register, subject to
configured input polarity. `read_outputs` reads commanded output-latch values;
it is not physical feedback. `read_polarity` and `read_configuration` read the
other registers. Every read selects its register and reads one byte in one
write-read transaction with a repeated START.

`write_outputs`, `write_polarity` and `write_configuration` replace an entire
register. `update_*` methods read back and replace only selected bits; values
outside the mask are ignored. A zero mask performs no I/O. A nonzero mask always
reads and writes, even if the value matches. `set_level`, `set_direction` and
`set_polarity` provide typed single-pin updates. Configuration bits use **1 for
input, 0 for output**. Output latches are push-pull when enabled; writing high is
not equivalent to high impedance. Set a pin to input to release it.

## Initialization, ownership and failure behavior

`new` leaves hardware untouched. `init(Config)` performs four independent writes:
all pins input, output latches, input polarity, requested directions. This loads
latches before enabling outputs even when attaching to an already configured
part. It temporarily releases all outputs to high impedance. Use it only when
the board permits that transition; it is neither atomic nor a hardware reset.
`Config::default()` contains the documented power-on writable-register values,
not an observation of the connected chip.

All operations return the original bus error. There are no hidden retries,
fallback samples or cached registers. Failed or cancelled writes may already
have changed hardware. Initialization stops at the first error, leaving any
completed prefix applied; even its failing final write may have taken effect.
The caller must recover the underlying bus as its HAL requires, then explicitly
read back or reapply state. `bus_mut()` permits recovery and `into_inner()`
returns the bus without changing outputs. Power loss cannot create a stale
software shadow, but it still resets device state and requires application
handling. No automatic reset detection is provided.

Keep **one logical owner per physical expander**, including across resets and
power changes. Masked updates have separate read and write transactions: a bus
proxy's transaction lock does not make the whole update atomic against another
driver or bus master. Share this driver under an application mutex held for the
entire method if multiple tasks need access. There are no independent pin handles
or synchronous `embedded-hal::digital` implementations hiding async I²C calls.

Reading the input register, including `read_pin`, can acknowledge INT for the
whole port. A read can acknowledge INT even if it subsequently errors or is
cancelled. INT is active low and open drain; it is not a lossless event queue.
The caller supplies any INT GPIO/pull-up and Embassy wait/timeout behavior.
Debounce, pin allocation, LED sink policy and power sequencing remain downstream.

## Controller panel example (Pico / Pico 2)

`examples/embassy.rs` uses the HC Controller's TCA9534 panel pinout. It reads all
five buttons, mirrors CCW/STOP/CW onto their respective LEDs, and logs changes
to all five button levels, including MINUS/PLUS. It is a focused driver exercise;
controller behavior, debounce and machine STOP policy remain downstream.
It stops on bus errors; an error can leave the previous LED state applied.

| Connection | Wiring |
| --- | --- |
| Bus | I²C0: GP8 → SDA; GP9 → SCL; RN9 provides 10 kΩ pull-ups to 3.3 V; 100 kHz |
| Supply/address | U5 TCA9534DWR at 3.3 V; A2/A1/A0 grounded → 0x20; INT unconnected |
| Buttons | CW=P0, STOP=P1, CCW=P2, MINUS=P6, PLUS=P7; external 10 kΩ pulls to 3.3 V, press to ground |
| LED sinks | CCW=P3, STOP=P5, CW=P4; +5V_SYS → LED → RN8 1 kΩ → port pin |

Initialization releases all ports, sets output latches and polarity to zero,
and leaves all LEDs off. The output latches remain **0x00** and polarity remains
**0x00**. The loop reads the whole input port, inverts the active-low button
levels in software and masks with **0xC7**. It uses
`update_configuration(0x38, !leds_on)` to change only LED directions:
**output LOW = on; input/high impedance = off**. Never drive these LED pins
push-pull HIGH. Button pins remain inputs; configuration is **0xFF** with all
LEDs off and **0xC7** with all three on. Simultaneous button presses light all
corresponding LEDs; this example does not arbitrate machine commands.

A nominal 1 ms ticker polls raw levels without debounce; bus I/O and logging can
overrun that interval, so it is not a STOP response guarantee. The 100 ms startup
delay is an example allowance, not a guarantee of board power sequencing.
The controller design uses an original Pico; the Pico 2 build checks API/linker
portability for the same GPIO wiring. Source pinout provenance is recorded in
[PROVENANCE.md](PROVENANCE.md).

```sh
cargo build -p polis-xca953xa --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release --locked
cargo build -p polis-xca953xa --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

Select one board per invocation. Normal library builds enable neither. Linked
ELFs are under the repository-root `.target/`; these commands do not flash hardware.
The archive contains its own build script and memory maps.

## Validation and distribution

Use Python 3.11 or newer for the matrix tool.

```sh
cargo test --locked -p polis-xca953xa
python3 tools/check.py build --platform all --package polis-xca953xa
python3 tools/check.py clippy --platform all --package polis-xca953xa
cargo doc --locked -p polis-xca953xa --no-deps
cargo package --offline --allow-dirty -p polis-xca953xa
```

Tests check wire transactions, address variants, initialization order, masked
updates, physical-input versus latch reads, power-reset readback, and failures
and cancellation before/after simulated hardware effects. Actual target builds
and linked examples validate compilation/integration, not electrical behavior.
See [VALIDATION.md](VALIDATION.md) for results and remaining hardware checks.

Original implementation under MIT; see [LICENSE-MIT](LICENSE-MIT).
