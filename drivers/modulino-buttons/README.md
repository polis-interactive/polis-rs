# polis-modulino-buttons

Allocation-free, `no_std` asynchronous I²C driver for **Arduino Modulino Buttons
(ABX00110)** running Arduino's stock firmware. Uses `embedded-hal-async` 1.0 and
selects no MCU or executor by default. Reads A/B/C and controls their three LEDs.

## Usage

```rust
use embedded_hal_async::i2c::I2c;
use polis_modulino_buttons::{Button, Error, ModulinoButtons};

async fn sample<B: I2c>(bus: &mut B) -> Result<bool, Error<B::Error>> {
    let mut buttons = ModulinoButtons::new(bus);
    let state = buttons.read_buttons().await?;
    buttons.set_leds(state.a, state.b, state.c).await?;
    Ok(state.is_pressed(Button::A))
}
```

`new` selects **0x3e**, the unshifted seven-bit address expected by HALs.
Arduino's **0x7c** is the shifted address/pinstrap identifier; do not pass it as
the bus address. `with_address(bus, address)` accepts an already configured
address in `0x08..=0x77`. It does not change the module's persistent address.
Constructors do not perform I/O. `into_inner` returns the bus; `bus_mut` permits
caller-managed recovery. Borrowed buses and HAL-compatible shared-bus adapters
are supported.

## Wire contract and limits

| Operation | Transfer | Meaning |
| --- | --- | --- |
| `read_buttons()` | One read of four bytes | `[0x7c, A, B, C]`; zero is released, nonzero is pressed |
| `set_leds(a, b, c)` | One write of three bytes | `[A, B, C]`; each is 0 (off) or 1 (on) |

Neither transfer has a register prefix. The pinstrap identifier remains `0x7c`
when the module uses a custom bus address; a different identifier returns
`Error::UnexpectedDevice`. Read once before writing to verify the module type.
Bus errors are preserved in `Error::I2c`; the driver does not retry or retain
stale button samples. No initialization command is required.

The firmware samples button GPIO levels on a read. Debounce, press/release
events, long presses, polling cadence and timeouts belong to the application.
Short presses between polls can be missed. Firmware processes LED writes in its
main loop; allow time between writes. There is no LED readback or brightness API.
Failed/cancelled writes may have affected LEDs: recover the bus as required by
the HAL and explicitly write all three desired states again. There are no
persistent-address programming, bootloader or bus-scanning operations.

The protocol is verified against the pinned Arduino library and firmware in
[PROVENANCE.md](PROVENANCE.md). It assumes stock firmware; custom firmware may
use a different protocol. Public API changes follow pre-1.0 SemVer.

## Pico / Pico 2 example

`examples/embassy.rs` polls every 20 ms, logs changed levels through RTT, and
mirrors the three button levels onto their LEDs. Polling does not provide
debounce. Its 100 ms startup allowance is illustrative; applications own startup
and recovery timing. Read failures skip LED updates; failed writes are retried
on the next successful poll. Product deadlines and bus recovery are not supplied
by this example.

| Modulino connection | Pico / Pico 2 connection |
| --- | --- |
| 3.3V | 3V3(OUT), physical pin 36 |
| GND | GND, for example physical pin 38 |
| SDA | GP6, physical pin 9 |
| SCL | GP7, physical pin 10 |

Use 3.3 V power and logic. Supply external pull-ups on SDA/SCL to 3.3 V, such as
4.7 kΩ if the bus does not already have suitable pull-ups. Arduino documents
unpopulated pull-up pads on this module. The example uses a 100 kHz bus.

```sh
# Pico (RP2040)
cargo build -p polis-modulino-buttons --release --target thumbv6m-none-eabi \
  --features example-rp2040 --example embassy

# Pico 2 (RP2350)
cargo build -p polis-modulino-buttons --release --target thumbv8m.main-none-eabihf \
  --features example-rp235xa --example embassy
```

Select exactly one example chip feature. Runtime dependencies, boot image and
package-local memory maps are enabled only for the example. ELF files are under
the repository-root `.target/<target>/release/examples/embassy`. These commands
build and link; they do not flash hardware.

## Validation

```sh
cargo test --locked -p polis-modulino-buttons
python3 tools/check.py build --platform all --package polis-modulino-buttons
python3 tools/check.py clippy --platform all --package polis-modulino-buttons
```

Mock-bus tests check exact addresses, read/write shapes, button and LED ordering,
custom addresses, identifier mismatches and error propagation. See
[VALIDATION.md](VALIDATION.md) for completed checks and hardware limits.

Original implementation: MIT; see [LICENSE-MIT](LICENSE-MIT).
