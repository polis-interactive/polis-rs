# MAX31865 RTD driver

`polis-max31865` is an allocation-free, `no_std`, async SPI driver for the
MAX31865, including PT100/PT1000 and two-, three-, and four-wire connections.
It uses `embedded-hal-async` 1.0 `SpiDevice` and `DelayNs`; the library selects
no MCU, executor, clock or allocator. The directory is named `max31865`, with
the collection's `polis-` prefix used for the Cargo package.

## Read a temperature

```rust
use embedded_hal_async::{delay::DelayNs, spi::SpiDevice};
use polis_max31865::{Config, Error, Max31865};

async fn temperature<S: SpiDevice<u8>, D: DelayNs>(
    spi: S, delay: &mut D,
) -> Result<f32, Error<S::Error>> {
    let mut sensor = Max31865::new(spi, Config::pt100_3wire())?;
    sensor.init().await?;
    sensor.read_temperature(delay).await
}
```

Configure the controller for **MSB first, SPI mode 1 or 3, at most 5 MHz**.
The `SpiDevice` implementation owns CS and bus locking; register address and
read bytes share one transaction. Its CS timing must meet the chip's requirements
(400 ns setup/inactive and 100 ns hold). Supply a borrowed device or a shared-bus
device adapter when appropriate. Use one logical driver per chip.

Construction validates calibration without I/O. `init()` applies wire/filter
settings, disables bias/continuous conversion, clears faults and sets the
thresholds to 0..32767 ADC counts. Use `Config::pt1000_3wire()` for a PT1000 with
a 4300 Ω reference, or set the actual fitted reference resistor and wiring in
`Config`. Software settings cannot replace the breakout's required RTD jumpers.

`read_rtd()` clears old faults, enables bias, waits 11 ms, triggers a one-shot,
waits 66 ms, reads the result and disables bias. The conversion wait covers both
filter settings. Input settling must cover 10.5 RC time constants plus 1 ms;
use `read_rtd_with_settling(delay, milliseconds)` for larger RC networks, followed
by `calculate_temperature(raw, nominal, reference)` as needed.

Temperature conversion retains Adafruit's quadratic Callendar–Van Dusen solution
and negative-temperature polynomial for IEC platinum RTDs. The approximation is
intended for -200..850 °C; calibration, lead resistance, reference accuracy and
sensor limits remain application concerns. `calculate_temperature` returns
`None` for invalid numeric inputs/results; it does not inspect wiring faults or
enforce the sensor's physical temperature range.

## Continuous measurements, thresholds and faults

For continuous operation, enable bias, wait for the input network to settle,
then call `auto_convert(true)`. Allow 55 ms (60 Hz) or 66 ms (50 Hz) for the first
result. `read_sample()` reads the latest conversion without waiting or triggering.
The application handles DRDY, freshness and scheduling. One-shot methods reject
continuous mode. `configure()` applies a new configuration with continuous mode
and bias off; it also clears faults but preserves thresholds.

`set_thresholds(lower, upper)` and both threshold getters use unshifted 15-bit
ADC counts, matching `read_rtd()`. The write validates the range/order and writes
both thresholds in one transaction. Endpoint thresholds still allow faults at
zero/full-scale; they do not disable fault detection.

`read_sample()`, `read_rtd()` and `read_temperature()` return `Error::Fault` when
the RTD fault bit is set, preserving the status register's flags. Even an empty
status with a faulted sample remains an error. `read_fault()` reads latched status
without changing it; `clear_fault()` clears it. Iterate `FaultStatus` and use
`fault_message` to describe all active flags.

`start_fault_cycle` explicitly issues automatic or manual detection commands.
Settle bias first. Automatic detection supports input RC time constants <=100 µs;
wait at least 600 µs or poll `fault_cycle_finished()` with a delay and deadline.
For manual detection, follow the datasheet's five-time-constant waits around
`ManualRun` and `ManualFinish`, then poll completion before reading status.
These commands leave bias enabled and continuous conversion disabled; the caller
restores its desired operating mode. Detection is separate from ordinary samples
and is needed to diagnose the fault types checked only on demand.

Register behavior and timing follow the
[MAX31865 datasheet, revision 3](https://www.analog.com/media/en/technical-documentation/data-sheets/MAX31865.pdf),
pages 3–4 and 13–19.

## Errors, cancellation and migration

`Error::Spi` preserves the device's original error. One-shot measurement attempts
bias shutdown even after a transport or sensor error; an original measurement
error takes precedence over a cleanup error. A cleanup-only error is reported.
No driver retries, bus timeouts, or bus recovery are hidden inside these methods.

Dropping an async operation cannot perform async cleanup. Bias, conversion or
fault detection may remain active after cancellation or a partial write. Recover
the SPI bus through `device_mut()`/`into_inner()` if needed, call `init()` or
reapply configuration, and allow 66 ms for an outstanding conversion before
starting another. Reapply custom thresholds after `init()`. Configuration getters
describe the last successfully applied request, not verified hardware state.

This is an adapted API, not a drop-in dependency rename:

- `Max31865` is always async; the blocking API and `async` feature are removed.
  `new()` is fallible, and `configure()` replaces the software-only `set_config`.
- `configure()` sets wiring/filter together; `into_inner()` replaces `free()`.
  `read_sample()` supports continuous operation.
- Threshold arguments are **15-bit counts**, not shifted 16-bit register values.
  Sensor faults now return errors instead of silently yielding a temperature.
- `read_fault()` reads status only. Use `start_fault_cycle()` and
  `fault_cycle_finished()` for detection; `FaultCycle::None` is unnecessary.
- `calculate_temperature()` returns `Option<f32>` to report invalid arithmetic.

## Minimal Embassy example

[`examples/embassy.rs`](examples/embassy.rs) reads a three-wire PT100 with a 430 Ω
reference once per second and logs Celsius or fault flags over RTT. Connect
GP18 to SCK, GP19 to MOSI/SDI, GP16 to MISO/SDO, GP17 to CS, and connect the
breakout's supply and ground to 3.3 V and ground. Set its RTD jumpers for three
wires. For a PT1000 breakout, change the configuration to `pt1000_3wire()`.
The example uses a 1 MHz mode-1 SPI bus and the standard async device adapter.

```sh
cargo build -p polis-max31865 --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release --locked
cargo build -p polis-max31865 --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

Choose one board feature. Its opt-in dependencies supply the executor, time driver,
panic logger, boot image and package-local memory layout. The ELF is
`.target/<target>/release/examples/embassy` in this workspace; builds do not flash hardware.
The example stops on transport errors; applications own recovery policy.

## Validation and distribution

```sh
cargo test -p polis-max31865 --locked
python3 tools/check.py build --platform all --package polis-max31865
python3 tools/check.py clippy --platform all --package polis-max31865
cargo package -p polis-max31865 --allow-dirty --locked
```

Host mocks suspend SPI operations and delays and check transaction boundaries,
wire bytes, timing, faults, errors, cancellation and temperature fixtures.
The embedded matrix builds libraries and links the Pico/Pico 2 examples.
No electrical behavior or physical RTD accuracy is qualified by these checks.

## Attribution and license

Adapted from Bruce Wilcoxon's
[`adafruit-max31865-rs`](https://github.com/broose-goose/adafruit-max31865-rs),
itself inspired by
[`Adafruit_MAX31865`](https://github.com/adafruit/Adafruit_MAX31865), written by
**Limor Fried/Ladyada for Adafruit Industries**. Credit to Adafruit for the
original driver, conversion algorithm and open-source hardware work. This package
is named for the MAX31865 chip and is not an official Adafruit package.

The source declares BSD-3-Clause, retained here; see
[LICENSE-BSD-3-Clause](./LICENSE-BSD-3-Clause) and [NOTICE](./NOTICE) for the retained
Adafruit notices. [PROVENANCE.md](PROVENANCE.md) records revisions, original hashes
and adaptation details; [CHANGELOG.md](CHANGELOG.md) records the API changes.
The workspace's final repository URL and registry ownership must be established
before publication, as with the other collection packages.
