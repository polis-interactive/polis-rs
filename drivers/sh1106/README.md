# Async SH1106 OLED driver

`polis-sh1106` drives common **128×64 SH1106 I²C modules** with visible RAM
columns 2 through 129. It is `no_std`, allocation-free, and runtime-independent.
Normal builds depend only on `embedded-hal-async` 1.0 and
`embedded-graphics-core` 0.4. Mousefood, Ratatui, Embassy and an allocator are
enabled only by the example features.

## Drawing and flushing

```rust
use embedded_graphics_core::{Pixel, draw_target::DrawTarget, geometry::Point,
                             pixelcolor::BinaryColor};
use embedded_hal_async::i2c::I2c;
use polis_sh1106::Sh1106Async;

async fn draw<B: I2c>(bus: B) -> Result<(), B::Error> {
    // Power and hardware reset must already be complete.
    let mut display = Sh1106Async::new(bus); // unshifted address 0x3C
    display.init().await?;
    display.draw_iter([Pixel(Point::new(64, 32), BinaryColor::On)]).unwrap();
    display.flush().await
}
```

Use `Sh1106Async::with_address(bus, Address::Secondary)` for 0x3D. The bus can
be owned, borrowed, or an async shared-bus device. Drawing is synchronous and
infallible; out-of-bounds pixels are ignored. `clear_buffer()` clears local pixels;
`DrawTarget::clear()` supports either binary color. Neither sends bus traffic.
`buffer()` exposes read-only page-major bytes for inspection.

The framebuffer contains 1,024 bytes. `flush()` always sends eight complete pages,
each with three address commands and a 129-byte data write including its control
byte. Your I²C implementation must support that write length. At 400 kHz this is
approximately 25 ms of bus time per frame, excluding software overhead and clock
stretching. There is no dirty tracking, double buffering, SPI support, rotation
API, or support for other panel geometries. Full transfers keep recovery simple;
changed-page/range writes would be a useful future optimization for measured bus
pressure. Async transfers allow other executor tasks to run, but this driver's
framebuffer cannot be drawn into while its flush holds a mutable borrow.

## Initialization and recovery

The implementation uses the [SH1106 V2.6 datasheet](https://www.displayfuture.com/Display/datasheet/controller/SH1106.pdf),
especially command descriptions on pages 19–31. Initialization configures the
internal DC-DC converter with `AD 8B`, retains the source module's remapped
segment/COM orientation and contrast, uploads the existing framebuffer while
the display is off, then enables it. A newly constructed display starts blank.
Do not use this initialization for modules requiring external panel voltage.

Applications own power sequencing, hardware reset, required delays, bus timeouts
and recovery. `init()` does not pulse RES or delay for supply stabilization.
Modules without onboard reset need an application-controlled reset sequence
following their module/controller specifications before `init()`.

I²C errors are returned unchanged; there are no automatic retries. A failed or
cancelled flush can partially update the panel but preserves local pixels. After
recovering the bus, call `flush()` again: every page's address is explicitly reset.
If initialization failed, configuration changed, or the panel reset, repeat
`init()` instead. Cancellation behavior during a transaction belongs to the bus.
`bus_mut()` and `into_inner()` allow caller-managed recovery. Do not create
multiple drivers that concurrently address the same panel.

## Mousefood / Ratatui example

[`examples/embassy.rs`](examples/embassy.rs) is a self-contained Pico/Pico 2
example. Connect module SDA to GP6, SCL to GP7, GND to ground, and power according
to the module's specifications (the example assumes a 3.3 V-compatible module).
Use pull-ups to 3.3 V and address 0x3C. The example assumes onboard power-on reset
and waits 100 ms before initialization; adapt reset/power timing for your module.

The screen displays `SH1106 + Ratatui`, `Mousefood`, and alternating `Tick`/`Tock`
once per second. The 6×10 font provides 21 text columns and six rows. A 32 KiB
example-owned heap supports Ratatui's terminal buffers. The driver uses no heap.

```sh
# Pico / RP2040
cargo build -p polis-sh1106 --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release --locked
# Pico 2 / RP2350A
cargo build -p polis-sh1106 --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

Choose one board feature per build. The ELF is
`.target/<target>/release/examples/embassy`. These commands build and link only;
flash with your normal board/debug-probe workflow. Examples stop on errors to
keep the usage path short; applications supply deadlines and recovery policy.

Mousefood 0.4 matches the source application's integration. The example uses
`ratatui-core` 0.1.2 directly for `Terminal` and the `Text` widget. The attempted
`ratatui` 0.30.2 dependency resolved `ratatui-widgets` 0.3.2, whose unconditional
`alloc::sync::Arc` in `block/shadow.rs` does not compile for RP2040. Using core
avoids that dependency while retaining the terminal/rendering API. Full widget
library compatibility on RP2040 is not claimed.

The integration sequence is essential:

```text
terminal.draw(...)                                  // render into local pixels
terminal.backend_mut().display_mut().flush().await  // transfer pixels over I²C
```

Keep the terminal alive between frames so Ratatui retains its previous frame.
If application code clears/changes the driver's framebuffer outside Ratatui,
also invalidate Ratatui's cached screen (for example with `terminal.clear()`)
before drawing again. Bus-flush retries alone do not require invalidation.

## Validation and scope

```sh
cargo test -p polis-sh1106 --locked
python3 tools/check.py build --platform all --package polis-sh1106
python3 tools/check.py clippy --platform all --package polis-sh1106
```

Host tests cover clipping, page bit layout, both addresses, exact initialization
and page transfers, failure at every transaction, retry and cancellation. The
matrix builds the library and links the UI example on both RP2040 and RP2350A.
No physical hardware validation has been performed for this extraction. Reset,
power-up appearance, orientation, edge pixels and sustained refresh require a
bench test on the actual module. The source product remains unchanged.

The published [`sh1106` 0.5.0](https://docs.rs/sh1106/0.5.0/sh1106/)
offers a broader synchronous API. This extraction retains the source's async
I²C integration; it is intentionally a small driver for this module geometry.

See [PROVENANCE.md](PROVENANCE.md) and [CHANGELOG.md](CHANGELOG.md) for source
records and migration differences. Licensed under [MIT](LICENSE-MIT).
