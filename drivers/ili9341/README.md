# Async ILI9341 display driver

`polis-ili9341` provides buffered **320×240 landscape or 240×320 portrait** RGB565/RGB332 drawing with async
display transfers. It reuses [`lcd-async` 0.1.3](https://docs.rs/lcd-async/0.1.3/lcd_async/)
for ILI9341 initialization, address windows, SPI, and framebuffer drawing.
An optional PIO + DMA 8-bit parallel adapter uses `polis-pio-i8080`.
There is no controller fork. The local adapter supplies orientation-aware geometry,
bounded transfers, resource release, and the synchronous `DrawTarget` interface
used by Mousefood. Normal builds are allocation-free `no_std` with no MCU/runtime
selection. Mousefood, Ratatui core and the heap are example-only options.
Embassy RP is also used by the optional parallel transport.

## Module and interface

The requested [Hosyond B09XHRKFMM listing](https://www.amazon.com/dp/B09XHRKFMM)
identifies the 2.4-inch SPI ILI9341 module. [Hosyond's support site](https://hosyond.com/)
links that model to the [MSP2402 documentation](https://www.lcdwiki.com/2.4inch_SPI_Module_ILI9341_SKU:MSP2402),
which specifies four-wire SPI, 3.3 V logic, and separate touch pins. This package
supports this LCD over SPI and exposes a separate adapter for 8-bit parallel
modules. **Touch input and SD-card support are not included.**
The controller's ability to support other bus modes does not establish that a
module exposes them. The Hosyond SPI module cannot use the parallel example.
Physical hardware has not been qualified by this package.

The panel's native geometry is 240×320. Initialization rotates its memory mapping
to expose 320×240. `Config::default()` selects 90° clockwise landscape, BGR panel
order and normal inversion. `Landscape::Flipped` selects 270° landscape.
Set `Config::mirror_horizontal` to `true` to correct left-to-right mirrored text
in either landscape direction; it defaults to `false`. `Config::rotation` then
applies an additional clockwise turn: `Rotation::Deg270` rotates the corrected
image 90° counterclockwise and exposes a 240×320 portrait drawing area.
It defaults to `Rotation::Deg0`. Both framebuffer formats report the selected
dimensions through `size()` and use matching row strides and upload windows.
`WIDTH` and `HEIGHT` remain the default landscape dimensions; framebuffer byte
counts are unchanged by rotation. These settings change controller address
mapping without copying or rearranging framebuffer pixels. Change
`Config::color_order` or `inversion` if your module requires it; the example's
colored labels help identify swapped red/blue channels. RGB666 and
runtime orientation changes are outside this package's current API.

## Buffered drawing

```rust
use embedded_hal::digital::OutputPin;
use embedded_hal_async::delay::DelayNs;
use polis_ili9341::{BUFFER_SIZE, Config, Ili9341Async, InitError, Interface};

async fn start<'a, DI, RST>(
    interface: DI,
    reset: RST,
    pixels: &'a mut [u8; BUFFER_SIZE],
    delay: &mut impl DelayNs,
) -> Result<Ili9341Async<'a, DI, RST>, InitError<DI::Error, RST::Error>>
where
    DI: Interface<Word = u8>,
    RST: OutputPin,
{
    Ili9341Async::new(interface, reset, pixels, Config::default(), delay).await
}
```

For SPI, construct `SpiInterface::new(spi_device, dc_pin)`. The SPI device owns
CS and may use a shared bus. SPI errors remain `SpiError::Spi`; DC pin errors
remain `SpiError::Dc`. Initialization also distinguishes reset pin failures.

The caller supplies **153,600 bytes (150 KiB)** for a single framebuffer. Use
static storage or deliberately allocated memory; avoid a large local temporary
on an MCU stack. The driver borrows this memory and preserves its existing pixels
during initialization. Supply zeroed memory for a black initial frame.

Drawing updates only local pixels, using embedded-graphics `Rgb565` colors.
Coordinates outside the visible rectangle are clipped. `clear_buffer()` clears
to black; `DrawTarget::clear(color)` fills any color. `buffer()` exposes row-major
RGB565 bytes, most significant byte first. Ratatui drawing is synchronous:

```text
terminal.draw(...)                                  // local framebuffer
terminal.backend_mut().display_mut().flush().await  // async display transfer
```

RGB565 `flush()` sends all pixels in eight-row strips: 30 in landscape or 40 in
portrait. Each strip sets the full-width window and its eight-row window, issues
memory-write-start and writes 5,120 landscape or 3,840 portrait pixel bytes.
The selected interface must accept that write size.
This avoids a second framebuffer or a conversion-copy buffer. Full pixel data
takes approximately 154 ms at the example's 8 MHz SPI rate, plus command and
software overhead. There is no dirty tracking, double buffering or synchronization
to the panel's scanout; tearing is possible. Updating changed regions is a future
optimization, not an implied feature of Ratatui's own cell-diff rendering.

## Initialization, errors and recovery

`new(...).await` pulses the supplied reset pin, awaits upstream reset/sleep-out
delays, configures the controller, and uploads the existing framebuffer. Power
must already be stable; power sequencing and backlight control belong to the
application. Upstream enables the panel before the upload, so keep a separately
controlled backlight off until construction succeeds if startup artifacts matter.
The example ties LED on, following the module instructions; startup RAM may flash.

Initialization consumes its interface/reset handles on error or cancellation.
Pass `&mut interface` and `&mut reset` when recovery requires keeping those
resources. After recovering the bus, repeat construction with the same buffer.
`release()` returns the interface, reset pin and original framebuffer for later
reinitialization; `SpiInterface::release()` returns the SPI device and DC pin.

Failed/cancelled flushes can partially update the panel but preserve local pixels.
Retry a complete flush after recovering the bus: every window is established
again. After controller reset or configuration loss, release and reconstruct the
driver instead. There are no hidden retries or deadlines. Cancellation safety
of in-flight DMA and CS cleanup belongs to the transport. The PIO transport
provides explicit abort/deselect/queue cleanup; the SPI path relies on its supplied
SPI device. Use one
logical driver per panel, since commands and pixel data span multiple transactions.

## Mousefood / Ratatui example

[`examples/embassy.rs`](examples/embassy.rs) uses an Embassy DMA-backed SPI device,
Mousefood 0.4, and Ratatui core 0.1.2. The 10×20 font gives 32 columns and 12 rows.
A static 150 KiB framebuffer and 64 KiB example-owned heap keep large allocations
off the stack and fit both Pico and Pico 2. Applications with more UI/state must
budget memory themselves. Full ratatui-widgets compatibility is not implied;
the example follows the repository's Ratatui-core pattern for RP2040 support.

| Module label | Pico / Pico 2 connection |
| --- | --- |
| SCK | GP18 (SPI0 clock) |
| SDI/MOSI | GP19 (SPI0 transmit) |
| CS | GP17 |
| DC/RS | GP20 |
| RESET | GP21 |
| GND | GND |
| VCC | Supply according to the module specification (documented 3.3–5 V input) |
| LED | 3.3 V, per module instructions; not driven directly by an MCU GPIO |
| SDO/MISO | Unconnected; this example does not read the display |
| T_CLK, T_CS, T_DIN, T_DO, T_IRQ | Unconnected; touch is not used |
| SD-card connector/signals | Unconnected; SD is not used |

Use 3.3 V logic even when powering the module from 5 V. These connections apply
to the documented Hosyond module; verify the labels on your board revision.
The example uses SPI mode 0 at 8 MHz and waits 100 ms for supply settling before
reset/init. It displays a title, red/green/blue labels and alternating Tick/Tock
text, with a one-second delay after each flush. Errors stop the example.

```sh
# Pico / RP2040
cargo build -p polis-ili9341 --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release --locked
# Pico 2 / RP2350A
cargo build -p polis-ili9341 --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

Select one board feature per invocation. The image is
`.target/<target>/release/examples/embassy`; these commands link but do not flash.
The package includes both memory maps and its linker build script.

## PIO 8-bit parallel example

Enable `rp2040` or `rp235xa` for `parallel::{PioI8080, Pins, BusConfig,
PioInterface}`. Wrap a configured bus with `PioInterface::new(bus)` and pass it
into the same `Ili9341Async::new` constructor used by SPI. The adapter reports
`lcd_async::interface::InterfaceKind::Parallel8Bit`; it does not fork controller
initialization. Ordinary library builds still select no MCU. Select exactly one
MCU feature per build.

[`examples/parallel.rs`](examples/parallel.rs) shows the same Mousefood/Ratatui
UI over PIO + DMA, using a 150 KiB framebuffer and 64 KiB example heap.
All three parallel examples enable `mirror_horizontal` for the HiLetgo shield
used during bring-up. Set it to `false` if your panel does not need this correction.

| LCD signal | Pico / Pico 2 connection |
| --- | --- |
| D0, D1, D2, D3, D4, D5, D6, D7 | GP0, GP1, GP2, GP3, GP4, GP5, GP6, GP7 |
| WR | GP8 |
| DC / RS | GP9 |
| CS | GP10 |
| RESET | GP11 |
| RD | Hold inactive high at 3.3 V; no reads |
| GND | Common ground |
| Power / backlight | According to the actual module specification |
| Touch / SD | Unconnected |

These are **LCD signal names**, not Arduino shield digital-pin numbers. The
HiLetgo B0722DPHN6 listing describes an 8-bit shield, but its exact revision,
controller marking, data-pin mapping and level-shifting circuit need bench
verification before wiring. Do not assume an Arduino-labelled 5 V supply means
5 V logic is safe for the Pico, or that 3.3 V signals satisfy every shield buffer.
Data GPIOs must be consecutive, in ascending D0..D7 order.

The example selects PIO0 SM0, IRQ flag 0 and DMA_CH0. It binds both interrupt
handlers and starts CS high before muxing the PIO pins. Seven PIO instructions
produce WR timing. The default instruction-clock limit is 12 MHz, giving at most
2 MB/s before overhead; it can be adjusted through `BusConfig::max_clock_hz`.
This value is six times the ideal byte rate, not an SPI frequency. Integer-divider
rounding and FIFO stalls reduce the achieved rate. The supported clock bound is
50 MHz, but actual shield/wiring timing must be measured before increasing speed.

`flush()` still sends the complete frame; PIO does not add dirty tracking or
tear-free commits. Completion waits for the final WR hold time. Cancellation
stops DMA and the SM, clears pending data, and restores CS/WR/DC high; retry the
full flush after cancellation, or reset/reinitialize after an interrupted
configuration command. The adapter's `release()` returns its idle bus. See the
`polis-pio-i8080` package README for the transport contract.

```sh
cargo build -p polis-ili9341 --example parallel --features example-rp2040 --target thumbv6m-none-eabi --release --locked
cargo build -p polis-ili9341 --example parallel --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

The workspace build tool links SPI, RGB565 parallel and RGB332 parallel examples
on both MCUs.

## RGB332 framebuffer with PIO expansion

Use `Ili9341Rgb332` with `parallel::PioRgb332Interface` and a
`[u8; RGB332_BUFFER_SIZE]` framebuffer. This path supports **8080 only**;
ordinary SPI and `Ili9341Async` retain their RGB565 behavior.

```rust,ignore
use polis_ili9341::{Config, Ili9341Rgb332, RGB332_BUFFER_SIZE};
use polis_ili9341::parallel::PioRgb332Interface;

// Construct bus and common as in examples/parallel_rgb332.rs.
let interface = PioRgb332Interface::new(bus, &mut common);
let mut display = Ili9341Rgb332::new(
    interface, reset, framebuffer, Config::default(), &mut delay,
).await?;
display.flush().await?;
```

Storage is **76,800 bytes (75 KiB)**, half the RGB565 framebuffer. Each byte is
`RRRGGGBB`. Drawing still accepts `Rgb565`, so existing embedded-graphics and
Mousefood code works: drawing truncates to the upper 3 red, 3 green and 2 blue
bits. Native RGB332 renderers can write `buffer_mut()` directly. The driver
retains no second framebuffer or conversion buffer.

Initialization and command parameters remain byte-exact and select RGB565 on
the LCD. Each RGB332 flush submits one borrowed 76,800-byte framebuffer slice;
the transport bounds its DMA transfers internally. `flush_rows(y..y + 16)`
uploads a full-width band; bounds are clipped and empty ranges do nothing.
PIO expands component bits by repetition (`abc -> abcab`, `abc -> abcabc`,
`ab -> ababa`), then emits RGB565 high byte first. White becomes `0xffff` and
black `0x0000`. The LCD still receives 153,600 pixel bytes per frame. Completion
waits for the last WR hold time; cancellation restores the original byte-mode
program and deselects the panel before restoring WR high.

Expansion uses **one SM, one DMA channel, one IRQ flag and 31 PIO instruction
slots** (7 raw + 24 expansion) on the same block. Budget that instruction memory
when placing other PIO devices. The raw RGB565 bus still uses only 7 slots.
There is no CPU expansion pass, per-pixel interrupt or lookup table. A 256-entry
RGB565 lookup would occupy 512 bytes, but PIO cannot index SRAM directly; a CPU
conversion buffer or a more elaborate DMA lookup scheme would add memory traffic
and resources. This implementation keeps the direct-DMA path. CPU load
and contention benefits have not been benchmarked on hardware.

Expansion takes 25 PIO cycles per pixel, versus 12 cycles for two raw bytes.
At an unchanged PIO clock it lowers the maximum pixel rate; memory savings do
not reduce the LCD's wire bandwidth. At 125 MHz system clock the conservative
12 MHz default gives an ideal 169 ms full-frame time (~5.9 fps). Requesting the
50 MHz limit gives 41.67 MHz after divider rounding, an ideal 46.1 ms (~21.7 fps).
At 150 MHz system clock with a 50 MHz request, the ideal limit is 38.4 ms
(~26 fps). Commands, interrupts, DMA contention and FIFO stalls add time.
These are calculated ceilings, not measured performance or a 60 fps guarantee;
verify the actual bus on a bench before increasing its clock.

[`examples/parallel_rgb332.rs`](examples/parallel_rgb332.rs) uses the same wiring
and Mousefood UI as `parallel.rs`, with a 75 KiB framebuffer and 64 KiB heap.
It selects `rotation: Rotation::Deg270` for a 90° counterclockwise turn from the
mirror-corrected landscape view, giving a 240×320 display and 24×16 text cells.
On a non-wireless Pico/Pico 2, its onboard LED flashes three times at startup,
then stays on during setup and toggles after each completed UI frame transfer
(approximately once per second). Three flashes followed by a steady LED indicate
that setup or frame rendering/transmission has not reached the heartbeat loop.
The heartbeat confirms firmware progress, not that the LCD accepted the writes.
Initialization milestones are also logged through defmt RTT with an SWD probe.

```sh
cargo build -p polis-ili9341 --example parallel_rgb332 --features example-rp2040 --target thumbv6m-none-eabi --release --locked
cargo build -p polis-ili9341 --example parallel_rgb332 --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

## Two-state-machine RGB332 pipeline

`parallel::PioRgb332PipelineInterface` overlaps conversion with LCD output:

```text
RGB332 framebuffer -> DMA -> converter SM -> DMA -> alternating RGB565 buffers
                                                 -> DMA -> output SM -> LCD
```

Construct `Rgb332Converter::new(&mut common, sm1, input_dma, output_dma)`, then
`PioI8080Rgb332Pipeline::new(raw_bus, converter, scratch)`, and wrap it with
`PioRgb332PipelineInterface::new(pipeline)`. Pass that interface to
`Ili9341Rgb332::new`. The two SMs can share one PIO block or occupy different
blocks. The [benchmark](examples/parallel_rgb332_bench.rs) shows complete wiring
and interrupt bindings for the same block.

The pipeline needs **two SMs, three DMA channels, one PIO IRQ flag, and 21 PIO
instructions** (7 output + 14 conversion). The caller supplies scratch memory:
`[u16; 2 * WIDTH * 8]` gives two eight-row buffers, totaling **10 KiB**. Combined
with the RGB332 framebuffer this is 85 KiB. Smaller buffers save memory but
increase scheduling overhead; each half is capped at 32,767 pixels. There is
no CPU pixel conversion or copying. The CPU schedules each chunk; this is not
an autonomous whole-frame DMA chain. Staging adds SRAM reads/writes, so contention
and CPU utilization need measurement under the intended application load.

The converter uses the system clock and 14 cycles per pixel. The output SM keeps
the configured integer divider and six cycles per byte. Separate FIFO-paced DMA
channels and exclusive buffer ownership prevent overruns without a fixed timing
relationship between the SMs. `flush()` uses one window for the full frame so
the buffers can alternate continuously; `flush_rows()` does the same for a band.
Only the first buffer must be converted before output starts. Cancellation aborts
active DMA, resets the stages and deselects the panel; retry the region afterward.

With a 50 MHz output-clock request and eight-row staging, the ideal full-frame
times, including first-buffer conversion but excluding software/DMA stalls,
are about **22.4 ms at 125 MHz system clock** and **18.7 ms at 150 MHz** (roughly
45 and 54 uploads/s). These are estimates, not measurements or panel scanout rates.
The unchanged one-SM path remains available for comparison and lower resource use.

### Hardware comparison benchmark

`parallel_rgb332_bench` first measures the one-SM path, frees its expansion
program, then measures the two-SM path on the same pins and output-clock setting.
It uses a precomputed 256-color grid and no heap. It times 30 full-frame uploads
and 150 rotating 320×16 band uploads per suite, printing average/minimum/maximum
microseconds and uploads/s × 1000 over defmt RTT. Rasterization, initialization,
warmup and logging are outside the timed regions; DMA setup, command writes,
executor wakeups and final WR hold time are included. Two-SM suites repeat;
reset the board to rerun the baseline. This measures latency, not CPU utilization.

Before timing the pipeline, the fixture cancels three full-frame transfers at
100, 500 and 5,000 µs and retries each. Verify the complete grid returns without
shifted pixels; use a logic analyzer to verify CS/WR cleanup. A successful build
or log does not establish electrical or cancellation correctness on hardware.

```sh
cargo build -p polis-ili9341 --example parallel_rgb332_bench --features example-rp2040 --target thumbv6m-none-eabi --release --locked
cargo build -p polis-ili9341 --example parallel_rgb332_bench --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

Flash `.target/<target>/release/examples/parallel_rgb332_bench` with your board's
normal debug-probe workflow and capture RTT from reset. Wiring matches the
parallel example above. The benchmark requests 50 MHz; use `BUS_MAX_CLOCK_HZ =
12_000_000` initially for an unqualified connection. `STAGING_ROWS` controls the
memory/scheduling tradeoff. Report the printed actual clocks with measurements.

## Rocket graphics example

[`parallel_rockets`](examples/parallel_rockets.rs) draws the supplied 11×21 rocket
sprite at 2× scale, with eight rotated headings, flickering exhaust and a twinkling
starfield. Each launch chooses a heading and speed, starts fully offscreen, crosses
the display, and despawns after exiting. Position uses elapsed time, so slower
LCD uploads do not slow the rockets' travel. It uses the same GP0–GP11 wiring,
horizontal mirror correction and 240×320 counterclockwise orientation as
`parallel_rgb332`.

Edit the constants at the top of the example:

| Setting | Default / meaning |
| --- | --- |
| `N` | Average visible population: 14, estimated as 18% of screen area divided by the scaled sprite's bounding-box area. Replace the expression with any desired integer. |
| `MIN_SPEED`, `MAX_SPEED` | 28–70 pixels/second, chosen independently per rocket. |
| `HEADINGS` | Allowed 45° steps clockwise from up: `0` up, `2` right, `4` down, `6` left. Restrict the slice to limit directions. |
| `SPRITE_SCALE`, `STAR_COUNT`, `BACKGROUND_RGB332` | Integer zoom (2), number of stars (80), background color. |
| `TARGET_FPS`, `BUS_MAX_CLOCK_HZ` | 30 FPS ceiling and a 50 MHz PIO output-clock request; actual FPS appears in the HUD. Reduce the clock request to 24 or 12 MHz if the panel shows corrupted pixels. |
| `MAX_ROCKETS` | Fixed storage capacity, including approaching/departing rockets. Defaults to `4*N+8`; overflow is counted in RTT. |
| `OFFSCREEN_MARGIN` | Clearance for exhaust before spawn/despawn; at least `7*SPRITE_SCALE`. |
| `WARMUP_SECONDS`, `RANDOM_SEED`, `SHOW_HUD` | Simulated pre-roll (30 seconds, not a wall-clock delay), repeatable random seed, FPS/population overlay. Use warmup 0 for an initially empty screen. |

The launch interval is each sampled visible lifetime divided by `N`, which targets
that long-run average rather than an exact simultaneous count. With `N = 14`, a
120-second host simulation after pre-roll averaged **14.16** visible rockets
(range 10–19) with no
capacity drops. Changing speed or allowed directions automatically changes the
launch cadence. `N = 0` gives a starfield only. Every boot repeats the seeded
sequence; edit `RANDOM_SEED` to choose another sequence.

The firmware uses a 75 KiB RGB332 framebuffer, 7.5 KiB of pipeline staging and
fixed sprite/scene storage, without a heap. White sprite background becomes
transparent; black outlines and pink shading remain. The two-SM/three-DMA pipeline
uploads full frames. `TARGET_FPS` is a ceiling, not a promised frame rate.

Initialization precomputes all eight rotations as horizontal runs of equal color,
including scaled offsets/widths and the exhaust positions. Rendering clips and
fills each run instead of scanning transparent pixels and scaling/clipping every
opaque pixel. The supplied sprite uses 41–70 runs per heading. The fixed cache
allows 96 runs per heading and checks that bound at startup. Simulation tests
compare the cached renderer against pixel rendering at all headings, three scales,
three flame lengths and screen edges. Frame-wide animation ticks are also shared
between sprites/stars; inactive rockets skip position calculations in the HUD.

The second HUD line reports per-frame averages in milliseconds: **U** updates the
simulation, **R** renders the framebuffer including the HUD, and **L** uploads the
frame, including conversion, DMA scheduling and waiting for LCD writes. The values
refresh once per second and also appear in RTT as `update_us`, `render_us` and
`lcd_us`. Frame pacing is excluded from those phases but included in FPS. These
are elapsed times, not CPU-utilization measurements.

At a 125 MHz system clock, the previous 12 MHz PIO request selects divider 11.
Sending 240×320 pixels at two bytes/pixel and six PIO cycles/byte takes at least
81.1 ms (about 12 uploads/s), regardless of rocket count. The 50 MHz request selects
divider 3, giving 41.67 MHz PIO and a 22.1 ms payload floor, plus initial conversion
and software/stall overhead. This changes only the bus divider, not the CPU clock.
The faster setting needs checking on the connected shield and wiring. Compare U/R/L
before considering a second core: overlapping rendering with upload could help
when R is substantial, but cannot shorten the LCD transfer itself.

For example, measured U/R/L values of 0/25/23 ms explain approximately 21 FPS.
A second framebuffer would cost another 75 KiB; rendering and uploading on
separate cores could ideally approach `max(U+R, L)` rather than `U+R+L`, with
additional synchronization and memory-contention overhead. This example currently
keeps one framebuffer/core and precomputes repeated drawing work. Compare the new
R reading before adding that coordination. Its frame-rate ceiling remains 30 FPS.

Build first, then connect the Pico while holding BOOTSEL and flash the linked ELF:

```sh
cargo build -p polis-ili9341 --example parallel_rockets --features example-rp2040 --target thumbv6m-none-eabi --release --locked
picotool load -v -x .target/thumbv6m-none-eabi/release/examples/parallel_rockets -t elf
```

Pico 2 build:

```sh
cargo build -p polis-ili9341 --example parallel_rockets --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

### Native preview and simulation tests

The same source can run on the host without Embassy or Cargo features. From the
repository root (create `.target/` first if this is a fresh checkout):

```sh
mkdir -p .target
rustc --edition=2024 -O drivers/ili9341/examples/parallel_rockets.rs -o .target/rocket-preview
.target/rocket-preview /tmp/polis-rockets
rustc --edition=2024 --test drivers/ili9341/examples/parallel_rockets.rs -o .target/rocket-tests
.target/rocket-tests
```

The preview prints population statistics and writes 90 PPM frames at simulated
15 FPS. It shares flight physics and pixel rendering with firmware; the hardware
FPS HUD and LCD transfer timing are excluded. Tests cover offscreen lifecycles,
speed/direction bounds, average population/capacity, frame-rate-independent
trajectories, clipped rendering and the zero-population case.

For an animated preview, optionally convert the frames with Python and Pillow:

```sh
python3 - <<'PY'
from pathlib import Path
from PIL import Image
frames = [Image.open(p) for p in sorted(Path('/tmp/polis-rockets').glob('frame-*.ppm'))]
frames[0].save('/tmp/polis-rockets/preview.gif', save_all=True,
               append_images=frames[1:], duration=[60, 70, 70] * 30, loop=0)
PY
```

## Validation

```sh
cargo test -p polis-ili9341 --locked
python3 tools/check.py build --platform all --package polis-ili9341
python3 tools/check.py clippy --platform all --package polis-ili9341
```

Host tests exercise the real upstream SPI adapter with suspending mock transfers
and pin/delay traces. They cover reset timing, landscape/portrait orientations and mirroring,
RGB565 byte order, clipping, exact strip windows/data, all flush SPI failure
positions, GPIO errors, initialization retry, cancellation and resource release.
RGB332 tests cover all 65,536 input RGB565 colors, native byte edits, clipping,
unmodified initialization/window commands, direct framebuffer slice addresses,
all flush failure positions and retry/release. The transport tests execute all
256 RGB332 expansions, WR timing, FIFO starvation and successive large transfers
from the assembled PIO programs. Converter tests independently stall input/output
FIFOs and check halfword byte-swap ordering and the 14-cycle conversion cost.
Row-upload tests verify full-frame/band windows, clipping and borrowed slices.
Target builds and linked examples do not qualify physical hardware. Initialization,
panel orientation/colors, signal integrity and sustained refresh need bench testing
on the actual module. Touch has no implementation or validation in this package.

See [PROVENANCE.md](PROVENANCE.md) and [CHANGELOG.md](CHANGELOG.md).
Licensed under [MIT](LICENSE-MIT).
