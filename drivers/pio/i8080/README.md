# PIO 8-bit 8080 display bus

`polis-pio-i8080` implements the write-only 8080-style **display** interface:
D0..D7, active-low WR and CS, and DC low for commands/high for data. It uses
Embassy RP 0.10, one PIO state machine, its matching IRQ flag (SM0/IRQ0, etc.),
seven instruction slots and one DMA channel. `polis-ili9341` is its first consumer.
An optional `PioI8080Rgb332` wrapper expands RGB332 pixels to RGB565 on the bus.
`PioI8080Rgb332Pipeline` instead overlaps expansion and output on two SMs.
There are no ILI9341 opcodes, dimensions, reset sequences, or
framebuffers in this package. This is not an Intel 8080 CPU bus implementation.

Enable `rp2040` or `rp235xa`; neither is selected by default. Normal library
builds need no allocator, executor or time driver. No-feature builds expose
`Config`/`ConfigError`, and host tests execute the assembled waveform.

## Pins and timing

Supply eight consecutive GPIOs in D0..D7 order and a separate WR GPIO using
`Common::make_pio_pin`. Supply DC and CS as Embassy GPIO outputs. **Create CS
high before muxing WR/data** so pin setup cannot write to the panel. Pin ownership
is exclusive; this API does not arbitrate a shared parallel bus. Hold RD high at
the panel's logic voltage (3.3 V for ILI9341). Reset belongs to the panel driver.
The eleven bus GPIOs exclude reset, backlight, RD, power and ground.

`Config::max_clock_hz` bounds the PIO instruction clock, not the byte clock.
An integer divider rounds down to avoid fractional-divider timing jitter.
Each byte takes six instruction cycles plus any FIFO stalls:

1. Pull a FIFO word with WR high. Starvation extends only this idle phase.
2. Drive D0..D7 with WR high for one cycle (data setup).
3. Hold WR low for two cycles.
4. Raise WR and hold for two cycles before looping or reporting completion.

The next iteration adds pull/setup cycles to the high phase. Data stays unchanged
for at least two cycles after latching. The maximum permitted instruction clock
is 50 MHz: setup before WR low >=20 ns, WR low >=40 ns, byte period >=120 ns.
These exceed ILI9341's 10 ns data setup/hold, 15 ns WR pulse and 66 ns cycle
minima. Other controllers must be checked against these timings and slowed as
needed. CS/DC setup includes the count-word preamble; completion follows the last
WR rising edge and two hold cycles. Do not change the system clock at runtime.

Default 12 MHz requests at most 2 MB/s. At stock 125 MHz RP2040 the divider is 11,
giving about 1.89 MB/s; at 150 MHz RP2350 the divider is 13, about 1.92 MB/s.
These are waveform ceilings, not measured throughput. Jumper wires, shield
level-shifting circuitry, and DMA contention can require slower operation.
See the [ILI9341 datasheet](https://files.waveshare.com/upload/e/e3/ILI9341_DS.pdf),
section 18.3, for electrical requirements.

## Transfers and cancellation

`write_command(command, args).await` keeps CS low across the command and its
parameters, waiting for the command's final WR pulse before changing DC.
`write_data(bytes).await` explicitly sets DC high. Empty data writes are no-ops.
Byte slices go directly to DMA; narrow FIFO writes replicate each byte and the
PIO consumes the low eight bits. Large slices are split at 65,535 bytes with CS
held low. The bus does not interpret or swap bytes.

DMA completion only means bytes reached the FIFO. The separate PIO completion
IRQ ensures the last byte has actually been strobed and held before CS changes
or the caller reuses its buffer. Both DMA and PIO IRQ handlers must be bound.

Dropping a pending write aborts DMA through Embassy's transfer destructor, then
disables the SM, deselects CS, clears queued bytes, resets the program counter and
restores WR/DC high. A cancelled transfer can have written partial data; nothing
rolls back panel memory. Reissue the entire address window before retrying pixels.
If a configuration command was interrupted, reset/reinitialize the controller.
No further queued bytes are emitted after cancellation cleanup returns. Dropping
the bus also deselects it. There is no deadline, readback, or electrical-error
detection; a missing interrupt handler can stall a future indefinitely.

## Optional RGB332 expansion

`PioI8080Rgb332::new(bus, &mut common)` adds an expansion program to an existing
idle bus. It retains the bus's pins, clock, SM, DMA and IRQ, and allocates 24
additional instructions (31 total). Construction panics if that instruction
space is unavailable. `common` must be from the same PIO instance.

`write_command` and `write_data` keep their byte-exact meanings.
`write_rgb332(pixels).await` reads packed `RRRGGGBB` bytes directly with byte-wide
DMA, using the replicated FIFO word to construct RGB565 in PIO. Channels expand
by bit repetition: `abc -> abcab`, `abc -> abcabc`, `ab -> ababa`. Each input byte
produces two WR strobes, high RGB565 byte first, in 25 instruction cycles plus
FIFO stalls. The panel driver must configure RGB565 and the destination window.

Data setup and hold are at least one and two cycles respectively; WR low is two
cycles. The two bytes of one pixel are five cycles apart (100 ns at the 50 MHz
limit). This still exceeds the ILI9341 timing minima above. DMA chunks contain
at most 65,535 **pixels**, with CS held low. After each expanded write or
cancelled future, the SM is restored to its raw-byte configuration. Completion
and cancellation keep the original contract. `release(&mut common)` returns the
idle raw bus and frees the expansion instructions. Ordinary Drop stops the bus
but does not reclaim Embassy's instruction allocation.

No lookup table or expanded framebuffer is needed. This saves source memory and
halves pixel DMA transfers, but the LCD receives the same number of bytes and
expansion adds PIO cycles. At a fixed instruction clock the pixel-rate ceiling
is `system_hz / (divider * 25)`. This is a throughput tradeoff for reducing CPU
work relative to software expansion, not a hardware throughput measurement.

## Two-SM pipeline

`Rgb332Converter::new(common, sm, input_dma, output_dma)` reserves a pinless SM
and 14 instructions. It runs at the system clock, taking 14 cycles per pixel.
Both DMA channels need their Embassy interrupt handlers bound; no converter PIO
IRQ is used. Combine it with the raw bus using
`PioI8080Rgb332Pipeline::new(bus, converter, scratch)`. The block may be the same
as the output SM's (21 instructions total), or a different PIO block.

The input DMA writes RGB332 bytes to the converter TX FIFO. A separate RX-paced
DMA reads RGB565 halfwords with hardware byte swap into one staging buffer.
Concurrently, the original bus DMA reads the preceding buffer's bytes, emitting
the high byte first through the unchanged output program. Every DMA uses the
DREQ of the FIFO it accesses; blocking PULL/PUSH handle starvation/backpressure.
Buffers change roles only after conversion and output both complete. This avoids
relying on one DREQ to pace both sides of a direct FIFO-to-FIFO transfer.

Supply a mutable `u16` slice: two halves become alternating buffers, each capped
at 32,767 pixels. At least two elements are required; any odd final element is
unused. For two 320×8 buffers use 5,120 elements (10 KiB). Resources total two
SMs, three DMA channels and the output SM's IRQ. The CPU schedules chunks without
converting/copying pixels. There is extra SRAM traffic compared with the single-SM
path; smaller buffers increase wakeup and setup overhead. No allocator is used.

`write_command`/`write_data` remain byte-exact. `write_rgb332` holds CS across
chunks, including pauses between them, and finishes after the last WR hold time.
Empty writes do nothing. Cancellation aborts each stage's DMA before resetting
its SM, clears queued data and deselects the display. Retry from a new address
window. The pipeline never needs simultaneous clocks or shared GPIOs between SMs.
Dropping it stops both SMs; Embassy's instruction allocations are not reclaimed.

The ideal sustained rate is `min(system_hz / 14, output_hz / 12)` pixels/s,
with one chunk's conversion latency at startup and scheduling overhead between
chunks. This is an analytical ceiling, not hardware qualification. The ILI9341
package supplies `examples/parallel_rgb332_bench.rs` to compare full frames and
320×16 bands with the one-SM path on the same bus.

## Examples and validation

`examples/embassy.rs` is a **logic-analyzer fixture**, not an LCD initializer.
Leave the display disconnected. It emits command 0x2c followed by bytes
00 01 80 55 aa ff every 100 ms. Pins: GP2..9 = D0..7, GP10 = WR, GP11 = DC,
GP12 = CS. Use `polis-ili9341/examples/parallel.rs` for the actual display and
Mousefood/Ratatui demonstration, including reset on GP13.

```sh
cargo test -p polis-pio-i8080 --locked
cargo build -p polis-pio-i8080 --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release --locked
cargo build -p polis-pio-i8080 --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

Instruction-level tests check byte order, FIFO starvation, single/multiple/large
transfers, WR setup/hold and completion after FIFO drain. Clock and pin validation
run on the host. RGB332 tests cover every color, black/white/primaries, repeated
transfers, maximum chunk counts, FIFO starvation and 25-cycle pixel timing.
These tests do not simulate pad timing, interrupt delivery or
DMA cancellation on silicon. Linked Pico/Pico 2 examples are build validation,
not hardware qualification. Bench checks must measure WR/data/CS/DC, confirm
display orientation/colors, and exercise cancellation/retry under DMA load.

Original MIT implementation; see [PROVENANCE.md](PROVENANCE.md),
[CHANGELOG.md](CHANGELOG.md), and [LICENSE-MIT](LICENSE-MIT).
