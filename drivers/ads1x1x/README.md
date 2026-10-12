# polis-ads1x1x

Allocation-free `no_std` async I²C driver for TI ADS1013/1014/1015 and
ADS1113/1114/1115, using `embedded-hal-async` 1.0. No MCU, executor or timer
is selected. The async API is always enabled; no feature flags are required.

| Models | Resolution | Inputs | Range | ALERT/RDY |
| --- | --- | --- | --- | --- |
| ADS1013 / ADS1113 | 12 / 16 bits | Fixed AIN0 − AIN1 | Fixed ±2.048 V | No |
| ADS1014 / ADS1114 | 12 / 16 bits | Fixed AIN0 − AIN1 | Programmable | Yes |
| ADS1015 / ADS1115 | 12 / 16 bits | Four-input multiplexer | Programmable | Yes |

Select the physical model explicitly; there is no chip ID probe. Other TI ADS
families, including the SPI ADS1018/ADS1118, require separate drivers.
Register definitions follow [TI ADS101x SBAS473F](https://www.ti.com/lit/ds/symlink/ads1015.pdf)
and [TI ADS111x SBAS444E](https://www.ti.com/lit/ds/symlink/ads1115.pdf).

## Async use

I/O methods have an `_async` suffix and accept an async bus/delay.
The consumer controls the enclosing deadline and bus recovery.

```rust,no_run
use polis_ads1x1x::{Address, Ads1015, Channel, Config, Error};
use polis_ads1x1x::model::DataRate12Bit;
use embedded_hal_async::{delay::DelayNs, i2c::I2c};

async fn sample<B: I2c>(bus: B, delay: &mut impl DelayNs) -> Result<i16, Error<B::Error>> {
    let mut adc = Ads1015::new(bus, Address::Ground);
    adc.configure_async(Config {
        data_rate: DataRate12Bit::Sps128,
        ..Default::default()
    }).await?;
    adc.read_one_shot_async(Channel::Single1, delay, 200).await
}
```

Rate types prevent selecting a 12-bit rate on a 16-bit model:

```rust,compile_fail
use polis_ads1x1x::{Config, model::{Ads1115, DataRate12Bit}};
let config = Config::<Ads1115> {
    data_rate: DataRate12Bit::Sps1600, // ADS1115 has a different rate table.
    ..Default::default()
};
```

## Conversion and recovery contracts

- Construction performs no I/O. `configure_async` applies complete settings and
  clears the OS command bit, so ordinary single-shot configuration cannot trigger
  an accidental conversion. Unsupported model settings fail before I/O.
- One-shot helpers require single-shot mode and check the device's current busy
  state. Bounded reads delay 1 ms before each poll, up to the supplied nonzero
  limit; 200 polls accommodates the slowest 8 SPS setting. Bus calls need their
  own timeout. Timeout/cancellation can leave a conversion running.
- Split `start_one_shot_async`, `one_shot_ready_async` and
  `read_conversion_async` methods support caller scheduling and GPIO waiting.
  Allow startup time before polling OS
  (TI specifies approximately 25 µs; the helper allows 1 ms). `true` also means
  idle before a trigger. Consumers own GPIO polarity, level/edge waiting and
  deadlines; avoid waiting only for an edge that may already have occurred.
- `read_conversion_async` returns the latest signed count, without proving
  freshness. Continuous-mode configuration changes can leave a conversion under old
  settings in flight; the caller must allow it to finish before relying on new
  channel/range data. Scaling requires the range that produced the sample.
- One driver owns each physical ADC. Configuration is never cached. Failed or
  cancelled transfers may partially change the device: recover I²C and reapply
  the full configuration. Configuring ALERT first disables it, programs both
  thresholds, then enables the requested function; this sequence is not atomic.

`Alert::Comparator` includes thresholds in signed model counts: −2048…2047 for
12-bit models and −32768…32767 for 16-bit models. `Alert::Ready` writes the special
raw threshold patterns itself. The ready output in continuous mode is a short
pulse; GPIO waiting is an application concern. ADC full-scale selection does not
permit input voltages outside the device's supply limits.

## Moving from ads1015-rs

This is an API redesign, not a drop-in rename. Replace the consumer's
`ads1015-rs` dependency with `polis-ads1x1x`, choose the model alias, and pass complete
`Config` values instead of cached field setters. `TargetAddr` becomes `Address`;
`Channel::SingleA1` becomes `Channel::Single1`. Async methods consistently use
`_async`; blocking methods are not provided. Ready-pin ownership and waiting move
to the consumer; alternatively, use the bounded async read with its timer
implementing `DelayNs`.

## Minimal Embassy example

[`examples/embassy.rs`](examples/embassy.rs) is a standalone Pico/Pico 2
usage example with all setup in one file.

Samples ADS1015 AIN0 once per second and logs signed counts over RTT.
Connect SDA to GP26, SCL to GP27, ADDR to ground (0x48), and use external
I²C pull-ups to 3.3 V. Supply the ADC at 3.3 V with a common ground; keep
AIN0 within the supply rails and the configured ±2.048 V range. The example
uses timer-based conversion polling; ALERT/RDY is not connected.

Build from the workspace root (choose one board):

```sh
# Pico / RP2040
cargo build -p polis-ads1x1x --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release --locked
# Pico 2 / RP2350A
cargo build -p polis-ads1x1x --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

The ELF is `target/<target>/release/examples/embassy`. Flash the matching
image with your usual SWD/UF2 workflow; an RTT-capable debug probe displays
`defmt` output. These commands build and link only; they do not flash hardware.
The opt-in features supply the executor, time driver, panic logger, boot image,
and package-local Pico/Pico 2 memory layout. Normal library builds keep their
existing runtime and MCU choices. Enable only one example board feature.
Examples stop on I/O errors with `unwrap()` to keep the usage path short;
applications own deadlines and recovery. Builds do not qualify hardware.

## Validation and release

```sh
cargo test -p polis-ads1x1x --locked
python3 tools/check.py build --platform all --package polis-ads1x1x
python3 tools/check.py clippy --platform all --package polis-ads1x1x
cargo package -p polis-ads1x1x --allow-dirty
```

Mocks cover all six models with yielding async I²C transactions. Hardware
qualification remains a consumer task. See `PROVENANCE.md` and `CHANGELOG.md`
in the package archive.
This package is licensed under MIT; see LICENSE-MIT. Package-name availability
is checked at publication.
