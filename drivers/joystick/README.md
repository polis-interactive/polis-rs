# polis-joystick

Allocation-free, `no_std` joystick calibration with interchangeable borrowed
async ADC sources. The core selects no MCU, executor, allocator, clock, pins,
ADC configuration, supply voltage, or calibration defaults.

## API and unchanged count semantics

| API | Contract |
| --- | --- |
| `AxisSource::read_axes(&mut self)` | Async complete `[i16; 2]` X/Y pair or the source's error |
| `Calibration::new(minimum, center, maximum, deadzone, inverted)` | Validates native-count calibration; returns `Option<Calibration>` |
| `Calibration::normalize(raw)` | Asymmetric, clamped normalization to -32767..=32767 |
| `Joystick::new([x_calibration, y_calibration])` | Stores explicit calibration; performs no I/O |
| `Joystick::sample(&self, &mut source)` | Async `Result<Reading, S::Error>` with `raw` and `position` X/Y arrays |

Calibration requires `minimum < center < maximum`, a nonnegative deadzone,
and a deadzone strictly smaller than each side's span. The inclusive deadzone
maps to zero. Outside it, subtract the deadzone from both the absolute offset
and the selected side's span, clamp the magnitude to that span, multiply by
32767, and divide using integer truncation. Apply the side's sign and optional
inversion last. There is no rounding to nearest and no -32768 normalized output.
Asymmetric endpoints, signed inputs and clipping are preserved from the source.

`Reading.raw` is never rescaled or clamped by this package. `Reading.position`
contains normalized values, not volts, millivolts, percentages, or ADC registers.

| Source | Native returned counts |
| --- | --- |
| ADS101x | Signed 12-bit -2048..=2047, right-aligned/sign-extended by polis-ads1x1x |
| ADS111x | Signed 16-bit -32768..=32767 |
| Embassy RP ADC | Unsigned 12-bit 0..=4095, represented exactly in `i16` |
| Custom `AxisSource` | The implementation's declared native `i16` count units |

Single-ended ADS acquisition commonly uses the nonnegative part of its signed
range, but negative noise/offset readings remain negative. Voltage scaling
depends on the application's selected ADC range/reference and physical supply.
Measure calibration in the same native units produced by the configured source.
No startup recentering, smoothing, hysteresis, button handling or product policy
is hidden in this package.

## Features and dependency selection

| Feature | Adds |
| --- | --- |
| none (default) | Portable trait, calibration, joystick and reading only |
| `ads1x1x` | `ads::AdsAxes`, polis-ads1x1x 0.1.0 and embedded-hal-async 1.0 |
| `rp-adc` | `rp::RpAxes`, Embassy RP 0.10.0; consumer must select an Embassy chip |
| `rp2040` / `rp235xa` | Convenience chip forwarding plus `rp-adc`; mutually exclusive |
| `example-rp2040` / `example-rp235xa` | Opt-in linked Pico/Pico 2 example and its runtime |

The two adapters are independently optional and can be enabled together.
Firmware can retain `rp-adc` and select its chip through its own embassy-rp
dependency, as the HDMI consumer does. Ordinary adapter features do not select
an executor, time driver, critical-section implementation or boot image.
The tested compiler is Rust 1.99.0.

`polis-joystick` is currently an uncommitted extraction, not a published release
or a package available at the existing polis-rs commit. Use a local Cargo patch
for integration validation; replace it with a real published version or reviewed
commit after that exists. Do not invent a Git revision for these changes.

## Two joysticks borrowing one ADS1015

`AdsAxes::new(&mut adc, &mut delay, [x_channel, y_channel])` takes short-lived
borrows. The caller owns ADC initialization, I2C address, range, data rate,
single-shot configuration, calibration, and recovery. A four-channel ADS1015
or ADS1115 can serve two joysticks sequentially without a mutex or allocation:

```rust,no_run
use embedded_hal_async::{delay::DelayNs, i2c::I2c};
use polis_ads1x1x::{Ads1015, Channel, Error};
use polis_joystick::{Calibration, Joystick, Reading, ads::AdsAxes};

async fn scan<B: I2c, D: DelayNs>(
    adc: &mut Ads1015<B>, // Already configured by the caller for single-shot mode.
    delay: &mut D,
    calibration: [[Calibration; 2]; 2],
) -> Result<[Reading; 2], Error<B::Error>> {
    let sticks = calibration.map(Joystick::new);
    let first = sticks[0].sample(&mut AdsAxes::new(
        adc, delay, [Channel::Single0, Channel::Single1],
    )).await?;
    let second = sticks[1].sample(&mut AdsAxes::new(
        adc, delay, [Channel::Single2, Channel::Single3],
    )).await?;
    Ok([first, second])
}
```

X completes before Y begins. The second source borrows the ADC after the first
sample's borrow ends. Channel/model compatibility is checked by polis-ads1x1x,
with its errors propagated unchanged. Do not construct duplicate driver objects
for one physical ADC. The BSP and this adapter must resolve **one copy/source
identity** of polis-ads1x1x so their `Ads1x1x` and `Channel` types agree.

## Borrowed RP ADC

`RpAxes` keeps the existing public `adc`, `x`, and `y` mutable-reference fields.
The application constructs its Embassy async ADC and analog channels:

```rust,ignore
use embassy_rp::adc::{Adc, Channel};
use embassy_rp::gpio::Pull;
use polis_joystick::{Joystick, rp::RpAxes};

// The application supplies p, an ADC_IRQ_FIFO binding, and measured calibration.
let mut adc = Adc::new(p.ADC, Irqs, Default::default());
let mut x = Channel::new_pin(p.PIN_26, Pull::None);
let mut y = Channel::new_pin(p.PIN_27, Pull::None);
let mut source = RpAxes { adc: &mut adc, x: &mut x, y: &mut y };
let stick = Joystick::new(calibration);
let reading = stick.sample(&mut source).await?;
```

The package's `examples/embassy.rs` is a complete linked Pico/Pico 2 example.
GP26/GP27 and its illustrative calibration belong only to that example.
Product pin/resource assignments and actual calibration remain downstream.

## Errors, cancellation and deadlines

A complete pair means both conversions succeeded; it does **not** mean the axes
were sampled simultaneously. If X fails, Y is not attempted. If Y fails, X is
discarded and no `Reading` is returned. The application can also await both
joysticks before publishing a complete four-axis snapshot, as in `scan` above.
The original source error type is returned unchanged; this library never
substitutes old samples, marks stale data fresh, retries, or updates stored state.

ADS acquisition preserves the original helper calls with a **200-poll limit per
axis**, each poll preceded by a 1 ms delay. This bounds status polling, not I2C
transaction duration or the complete pair's elapsed time. Wrap the entire sample
or scan in a caller-owned deadline. RP acquisition awaits two HAL conversions
sequentially, with caller-owned deadlines as well.

Dropping a pending sample future returns no reading and releases its borrows.
It does not roll back I/O: ADS configuration/MUX writes may have happened and
a conversion may still be running. The same caution applies to an in-flight RP
conversion. Recovery must follow the bus/HAL contract; a new sample is not a
promise that the hardware was reset or restored after cancellation. The caller
owns configuration reapplication, bus recovery, retries and validity timestamps.

## Build and validation

```sh
cargo test --locked -p polis-joystick
cargo test --locked -p polis-joystick --features ads1x1x
python3 tools/check.py build --platform all --package polis-joystick
python3 tools/check.py clippy --platform all --package polis-joystick
```

The matrix tests the portable core and ADS feature on the host; each embedded
target builds the RP adapter both alone and with ADS, and links the example.
Direct example builds:

```sh
cargo build --locked -p polis-joystick --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release
cargo build --locked -p polis-joystick --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release
```

Package together with the unpublished internal dependency for Cargo's temporary
registry verification; this does not publish either package:

```sh
cargo package --locked --allow-dirty -p polis-ads1x1x -p polis-joystick
cargo package --locked --allow-dirty -p polis-ads1x1x -p polis-joystick --features polis-joystick/ads1x1x
```

Calibration, mock-source, ADS channel ordering/range, shared-ADC and second-axis
failure tests were retained, with extra rounding/first-axis-failure assertions.
See [VALIDATION.md](VALIDATION.md) for actual results and limitations.
**Hardware qualification is unverified:** neither host tests nor linked firmware
establish wiring, noise, calibration, timing or physical cancellation recovery.

## Extraction and compatibility

The Rust interface and acquisition/normalization behavior are unchanged from
`hdmi-joystick`. Consumers replace the Cargo key and Rust import with
`polis-joystick` / `polis_joystick`; existing `ads1x1x` and `rp-adc` feature names
remain supported. Chip-forwarding and example features are additive.
No product firmware, BSP, control-link payload, or default calibration was moved
into the library API. See [PROVENANCE.md](PROVENANCE.md) and [CHANGELOG.md](CHANGELOG.md).
