# Async SSD1306 for 128×32 OLEDs

`polis-ssd1306` is an allocation-free, `no_std` I²C adapter for **128×32 SSD1306
modules**. It uses [`ssd1306` 0.10's existing async implementation](https://docs.rs/ssd1306/0.10.0/ssd1306/struct.Ssd1306Async.html)
for controller configuration and transport. No upstream fork or new async
implementation is needed. The public draw/flush API follows `polis-sh1106`.

The adapter owns one 512-byte framebuffer and sends complete frames. This keeps
pixels available for retries after failed or cancelled transfers. It deliberately
does not use upstream's buffered dirty-region flush: in 0.10.0 that method clears
dirty bounds before its first awaited write. Reusing only the upstream controller
with our local buffer avoids silently skipping a retry after interrupted I/O.
This is a narrow adapter, not a replacement for upstream's broader size, rotation
and SPI support.

## Usage

```rust
use embedded_graphics_core::{Pixel, draw_target::DrawTarget, geometry::Point,
                             pixelcolor::BinaryColor};
use embedded_hal_async::i2c::I2c;
use polis_ssd1306::{DisplayError, Ssd1306Async};

async fn draw<B: I2c>(bus: B) -> Result<(), DisplayError> {
    // Application has completed power stabilization and hardware reset.
    let mut display = Ssd1306Async::new(bus); // unshifted address 0x3C
    display.init().await?;
    display.draw_iter([Pixel(Point::new(127, 31), BinaryColor::On)]).unwrap();
    display.flush().await
}
```

Use `with_address(bus, Address::Secondary)` for 0x3D. Supply an owned bus,
`&mut bus`, or an async shared-bus device implementing `embedded-hal-async` 1.0.
Construction and drawing perform no I/O. Drawing clips to x=0..127 and y=0..31;
`clear_buffer()` clears pixels locally, and `DrawTarget::clear()` supports either
binary color. `buffer()` exposes the 512 page-major bytes read-only.

Call `init()` before normal use. Upstream selects the 32-row multiplex ratio,
sequential COM pin configuration, internal charge pump and default orientation
(`Rotate0`). Initialization preserves and uploads the local framebuffer. Upstream
enables the display before that upload, so stale controller RAM may briefly be
visible at startup. Applications own power, hardware reset, stabilization delays
and module-specific requirements; modules needing external panel voltage are
outside this adapter's initialization contract.

Each `flush()` restores horizontal addressing, selects columns 0–127 and pages
0–3 (no SH1106 column offset), then sends all 512 bytes. The upstream I²C interface
uses 32 data writes of 17 bytes each, including a 0x40 control prefix, plus three
command writes. At 400 kHz the wire time is about 13.3 ms, excluding software
overhead and clock stretching. There is no dirty tracking or double buffering.

## Errors and recovery

The re-exported `DisplayError` is upstream's error type. Its I²C adapter maps bus
errors to `BusWriteError`; it does not retain the underlying HAL error value.
Drawing is infallible. There are no implicit retries or deadlines.

A failed or cancelled transfer can partially change panel RAM. Local pixels stay
intact and the next flush rewrites the addressing mode, window and whole frame.
If initialization failed or the panel lost configuration, call `init()` again
after bus recovery. Bus cancellation safety and timeout behavior belong to the
bus implementation. Keep only one driver per physical panel.

`into_inner()` releases the bus. It consumes the driver and its framebuffer;
copy `*display.buffer()` first if pixels must survive reconstruction. For recovery
without releasing the driver, supply an application-owned shared-bus adapter with
an external recovery handle. There is no direct `bus_mut()` because upstream's
controller/interface owns the bus privately.

## Mousefood / Ratatui example

[`examples/embassy.rs`](examples/embassy.rs) targets Pico/Pico 2. Connect SDA to
GP6, SCL to GP7, GND to ground, and supply the module according to its specifications.
The example assumes a 3.3 V-compatible module with onboard power-on reset,
pull-ups to 3.3 V, and address 0x3C. It waits 100 ms before initialization; adapt
the reset/power sequence for the actual module.

The screen shows `SSD1306 + Ratatui`, `Mousefood`, and alternating `Tick`/`Tock`
once per second. A 6×10 font gives **21 columns and three rows** on the 128×32
panel. The example supplies a 32 KiB heap for the UI. The driver itself has no
allocator or runtime dependency.

```sh
# Pico / RP2040
cargo build -p polis-ssd1306 --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release --locked
# Pico 2 / RP2350A
cargo build -p polis-ssd1306 --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

Choose one board feature. The ELF is `.target/<target>/release/examples/embassy`.
These commands build and link; they do not flash hardware. The example stops on
errors; applications own retry/deadline policy.

Mousefood 0.4 and Ratatui-core 0.1.2 are optional dependencies enabled only by
example features. Core supplies `Terminal` and `Text`, matching the SH1106 example
without pulling in the RP2040-incompatible `alloc::sync::Arc` use observed in
ratatui-widgets 0.3.2. Full widget-library compatibility is not claimed.

Rendering and I²C transfer are separate:

```text
terminal.draw(...)                                  // synchronous local drawing
terminal.backend_mut().display_mut().flush().await  // asynchronous I²C upload
```

Retain the terminal between frames. If pixels are changed outside Ratatui,
invalidate its cached screen with `terminal.clear()` before rendering again.
Retrying an interrupted I²C flush needs no cache invalidation.

## Validation

```sh
cargo test -p polis-ssd1306 --locked
python3 tools/check.py build --platform all --package polis-ssd1306
python3 tools/check.py clippy --platform all --package polis-ssd1306
```

Host tests cover 32-row configuration, both addresses, clipping, byte layout,
full-frame transfers, error/cancellation retries and borrowed-bus release.
The matrix builds libraries and links the UI example for RP2040 and RP2350A.
No physical display has been tested for this addition. Module reset, orientation,
edge pixels, power-up appearance and sustained refresh still need bench validation.

See [PROVENANCE.md](PROVENANCE.md) and [CHANGELOG.md](CHANGELOG.md).
Original adapter code is [MIT](LICENSE-MIT). Upstream `ssd1306` remains a separate
dependency licensed MIT OR Apache-2.0.
