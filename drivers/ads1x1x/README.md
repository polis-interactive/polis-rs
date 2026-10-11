# ads1x1x

Allocation-free `no_std` I²C driver for TI ADS1013/1014/1015 and
ADS1113/1114/1115. Blocking and async methods use `embedded-hal` 1.0 and
`embedded-hal-async` 1.0 respectively. No MCU, executor or timer is selected.

| Models | Resolution | Inputs | Range | ALERT/RDY |
| --- | --- | --- | --- | --- |
| ADS1013 / ADS1113 | 12 / 16 bits | Fixed AIN0 − AIN1 | Fixed ±2.048 V | No |
| ADS1014 / ADS1114 | 12 / 16 bits | Fixed AIN0 − AIN1 | Programmable | Yes |
| ADS1015 / ADS1115 | 12 / 16 bits | Four-input multiplexer | Programmable | Yes |

Select the physical model explicitly; there is no chip ID probe. Other TI ADS
families, including the SPI ADS1018/ADS1118, require separate drivers.
Register definitions follow [TI ADS101x SBAS473F](https://www.ti.com/lit/ds/symlink/ads1015.pdf)
and [TI ADS111x SBAS444E](https://www.ti.com/lit/ds/symlink/ads1115.pdf).

## Blocking use

```rust,no_run
use ads1x1x::{Address, Ads1115, Channel, Config, Error, FullScaleRange};
use ads1x1x::model::DataRate16Bit;
use embedded_hal::{delay::DelayNs, i2c::I2c};

fn sample<B: I2c>(bus: B, delay: &mut impl DelayNs) -> Result<i32, Error<B::Error>> {
    let mut adc = Ads1115::new(bus, Address::Ground);
    adc.configure(Config {
        data_rate: DataRate16Bit::Sps128,
        range: FullScaleRange::V2_048,
        ..Default::default()
    })?;
    let counts = adc.read_one_shot(Channel::Single0, delay, 200)?;
    Ok(Ads1115::<B>::microvolts(counts, FullScaleRange::V2_048))
}
```

## Async use

The same operations have an `_async` suffix and accept an async bus/delay.
The consumer controls the enclosing deadline and bus recovery.

```rust,no_run
use ads1x1x::{Address, Ads1015, Channel, Config, Error};
use ads1x1x::model::DataRate12Bit;
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
use ads1x1x::{Config, model::{Ads1115, DataRate12Bit}};
let config = Config::<Ads1115> {
    data_rate: DataRate12Bit::Sps1600, // ADS1115 has a different rate table.
    ..Default::default()
};
```

## Conversion and recovery contracts

- Construction performs no I/O. `configure` applies complete settings and clears
  the OS command bit, so ordinary single-shot configuration cannot trigger an
  accidental conversion. Unsupported model settings fail before I/O.
- One-shot helpers require single-shot mode and check the device's current busy
  state. Bounded reads delay 1 ms before each poll, up to the supplied nonzero
  limit; 200 polls accommodates the slowest 8 SPS setting. Bus calls need their
  own timeout. Timeout/cancellation can leave a conversion running.
- Split `start_one_shot`, `one_shot_ready` and `read_conversion` methods support
  caller scheduling and GPIO waiting. Allow startup time before polling OS
  (TI specifies approximately 25 µs; the helper allows 1 ms). `true` also means
  idle before a trigger. Consumers own GPIO polarity, level/edge waiting and
  deadlines; avoid waiting only for an edge that may already have occurred.
- `read_conversion` returns the latest signed count, without proving freshness.
  Continuous-mode configuration changes can leave a conversion under old
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
`ads1015-rs` dependency with `ads1x1x`, choose the model alias, and pass complete
`Config` values instead of cached field setters. `TargetAddr` becomes `Address`;
`Channel::SingleA1` becomes `Channel::Single1`. Async methods consistently use
`_async`. Ready-pin ownership and waiting move to the consumer; alternatively,
use the bounded async read with its timer implementing `DelayNs`. The original
UTSuites product checkout is not changed by this extraction.

## Validation and release

```sh
cargo test -p ads1x1x --locked
python3 tools/check.py build --platform all --package ads1x1x
python3 tools/check.py clippy --platform all --package ads1x1x
cargo package -p ads1x1x --allow-dirty
```

Mocks cover all six models and both APIs. Hardware qualification remains a
consumer task. See `PROVENANCE.md` and `CHANGELOG.md` in the package archive.
This package is licensed under MIT; see LICENSE-MIT. Package-name availability
is checked at publication.
