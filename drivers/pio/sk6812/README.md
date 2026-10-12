# SK6812 PIO driver

`polis-pio-sk6812` implements the physical RGBW waveform for
`SKC6812RGBW-NW` and `SK6812MINIRGBW-NW-P6`. It is adapted from Embassy RP
0.10.0's `RgbwPioWs2812`; upstream attribution is retained under [MIT](LICENSE-MIT).

Enable exactly one MCU feature: `rp2040` or `rp235xa`. The application supplies
Embassy's time driver. BSP supplies the pin, PIO state machine, DMA channel and DMA
interrupt binding. No board pin numbers or chain mapping live here.

Load `PioSk6812Program` once per PIO block and pass it to all chain constructors.
It occupies four shared instructions. Each `PioSk6812<PIO, SM, N>` uses one state
machine, one DMA channel and one output pin. `write(&[RGBW<u8>; N])` packs pixels
in GRBW order into a `4 * N` byte inline buffer in the future. Preserve exported
chain order in the caller. Writes on separate chains may run concurrently.
The driver also implements `polis_led_output::LedOutput`: its slice API
rejects incorrect lengths before I/O. The fixed-array API remains available.

## Timing and completion

At 800 kbit/s, the waveform uses 20 PIO cycles per bit: nominal zero-high
312.5 ns, one-high 750 ns, zero-low 937.5 ns and one-low 500 ns. The divider is
rounded upward. `clk_sys` must be at least 32 MHz and must not change while the
driver exists. Both selected parts require a symbol period of at least 1.2 us
and reset low for **more than 200 us**:
[SKC6812RGBW datasheet, p. 9](https://www.ledlightinghut.com/files/SKC6812RGBW-XW.pdf),
[SK6812MINIRGBW datasheet, p. 8](https://datasheet.lcsc.com/datasheet/pdf/a2bc892b0af22abe04f5c8a837afb921.pdf?productCode=C7423107).

DMA completion only means the last word reached the FIFO. After DMA, the driver
waits a calculated upper bound for eight FIFO words, the shift register and one
in-flight bit, plus 250 us reset. At the default 125 MHz this is 612 us. This
avoids a busy loop and does not require an extra PIO interrupt. `write` returns
after transmission and reset, with data held low.

The initial write also waits this interval to establish reset. Cancelling a
write aborts its DMA but can leave a partially displayed frame. The next write
waits the complete drain/reset interval before sending new data. This prevents
the next frame from being appended to a cancelled frame. Normal consecutive
writes incur only the wait at the end of each write. The driver cannot detect
DMA starvation; the application must keep DMA supplied without long mid-frame
gaps. Dropping the driver mid-frame disables its state machine; initialize the
pin low before reusing it. Construction preloads the owned PIO output low before
pin mux handover and enables a data-pin pull-down afterward. This uses Embassy's
`unstable-pac` feature for the RP2040/RP2350A register setup. Handover and
power/reset transitions still require hardware qualification at the translator input.

## Minimal Embassy example

[`examples/embassy.rs`](examples/embassy.rs) is a standalone Pico/Pico 2
usage example with all setup in one file.

Blinks the white channel of eight SK6812 RGBW LEDs every 500 ms using
PIO0 SM0 and DMA_CH0. Connect GP2 through a suitable level shifter to
chain DIN; use an appropriately sized LED supply and common ground.
Set `LED_COUNT` to your chain length. Brightness is deliberately low.

Build from the workspace root (choose one board):

```sh
# Pico / RP2040
cargo build -p polis-pio-sk6812 --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release --locked
# Pico 2 / RP2350A
cargo build -p polis-pio-sk6812 --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
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
cargo test -p polis-pio-sk6812
cargo build -p polis-pio-sk6812 --features rp2040 --target thumbv6m-none-eabi --release
cargo build -p polis-pio-sk6812 --features rp235xa --target thumbv8m.main-none-eabihf --release
```

Host tests interpret the emitted PIO instructions to check MSB-first output,
branch pulse lengths, GRBW packing, timing bounds with fractional divider jitter,
and the drain/reset allowance. The target build compiles the library. It does
not demonstrate electrical behavior.

On hardware, measure each chain at the LED inputs: zero/one widths,
GRBW/white order, reset duration, consecutive frames, and cancellation/restart
under concurrent DMA load. Check PIO instruction capacity when adding programs.

## Source and distribution

See [PROVENANCE.md](PROVENANCE.md) for source hashes, extraction changes and
license status.
