# polis-74hc595

Allocation-free, `no_std` async SPI driver for 74HC595 and 74HCT595 shift registers.
Use MSB-first SPI mode 0 with MOSI on DS, SCK on SHCP, and a separate latch
output on STCP. Keep MR high. If OE is controllable, hold it high until the
first frame is latched.

```rust
# use embedded_hal::digital::OutputPin;
# use embedded_hal_async::{delay::DelayNs, spi::SpiBus};
use polis_74hc595::Hc595;
# async fn example<S: SpiBus<u8>, L: OutputPin, D: DelayNs>(spi: S, latch: L, delay: D)
# -> Result<(), polis_74hc595::Error<S::Error, L::Error>> {
let mut chain = Hc595::<_, _, _, 2>::new(spi, latch, delay)?;
chain.write_outputs(&[0x80, 0x01]).await?; // Farthest Q7 and nearest Q0 high.
# Ok(())
# }
```

Send one byte per chip, farthest first. Each write flushes the bus and latches
the entire frame. Hold the bus exclusively until the write completes. After an
error or cancellation, recover the bus and resend a complete frame.

## Controller counter

The Embassy example counts **00–99** once per second using 1 MHz PIO SPI.
On the Controller, GP15 feeds U4 DS, GP13 clocks both chips, and GP14 latches
them. U4 feeds U3. The example sends U3’s top-row segments first, followed by
U4’s bottom-row segments. Power the Controller from its regulated 5 V input.

```sh
DEFMT_LOG=info cargo build -p polis-74hc595 --locked --release --target thumbv6m-none-eabi \
  --features example-rp2040 --example embassy
probe-rs run --chip RP2040 .target/thumbv6m-none-eabi/release/examples/embassy
```

For Pico 2, build with `--target thumbv8m.main-none-eabihf --features example-rp235xa`.
