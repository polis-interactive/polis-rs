# Async TMP102 / TMP112 temperature sensors

`polis-tmp1x2` is a portable `no_std` driver over
[`embedded_hal_async::i2c::I2c` 1.0](https://docs.rs/embedded-hal-async/1.0.0/embedded_hal_async/i2c/trait.I2c.html).
It has no dependency on Embassy, an executor, or a particular I²C implementation.
Both an Embassy hardware I²C bus and the [PIO I²C bus](../pio/i2c/README.md)
can provide this standard trait. The caller may supply an owned bus, `&mut bus`,
or an async shared-bus adapter. Use one logical driver per sensor.

The implementation follows TI's
[TMP102 datasheet, SBOS397I](https://www.ti.com/lit/ds/symlink/tmp102.pdf),
sections 6.4–6.5, and
[TMP112 datasheet, SBOS473L](https://www.ti.com/lit/ds/symlink/tmp112.pdf),
sections 7.4–7.5. Their implemented measurement registers, configuration bits,
addresses and sample formats match, so `Tmp1x2` works with either chip without
a model flag or automatic identification. Sensor accuracy and conversion timing
must still be taken from the selected part's specification. TMP117 and TMP119
are not supported by this driver.

## Reading temperature

```rust
use embedded_hal_async::i2c::I2c;
use polis_tmp1x2::{Address, Config, ConversionRate, Error, Tmp1x2};

async fn read<B: I2c>(bus: &mut B) -> Result<i32, Error<B::Error>> {
    let mut sensor = Tmp1x2::new(bus, Address::Ground); // unshifted 0x48
    sensor.configure(Config {
        conversion_rate: ConversionRate::Hz1,
        ..Config::default()
    }).await?;
    // Reads the latest result, not necessarily a new conversion. The application
    // must allow the initial conversion to finish before using the first sample.
    Ok(sensor.read_temperature().await?.milli_celsius())
}
```

Construction performs no I/O. Each register read uses one `write_read`
transaction: pointer write, repeated START, two-byte read. No assumption is made
about the sensor's previous register pointer. All four ADD0 addresses are typed
as `Address::{Ground, Supply, Sda, Scl}`.

`Temperature` preserves signed 1/16 °C units. `sixteenths_celsius()` and
`micro_celsius()` are exact; `milli_celsius()` truncates toward zero (a -0.0625 °C
sample becomes -62 mC). Decoding uses D0 in the returned temperature register to
distinguish normal and extended samples, so there is no cached format to become
stale after a sensor reset or a partial configuration write.

## Configuration and one-shot conversions

`read_config()` reads measurement settings from the device. `configure()` sets
continuous conversion rate, shutdown and extended mode while preserving the
fault queue, alert polarity and thermostat mode. It never echoes the OS status
bit into a new one-shot trigger. Read-only/reserved bits are written as zero.
Configuration changes do not wait for a fresh conversion.

For one-shot operation:

1. Configure `shutdown: true`.
2. Poll `one_shot_ready()` with an application delay and deadline to allow any
   previous conversion to finish.
3. Call `start_one_shot()`.
4. Poll readiness again, then call `read_temperature()`.

`start_one_shot()` reports `NotShutdown` in continuous mode and `Busy` while a
conversion is active. `one_shot_ready()` performs one register read; it does not
wait internally. A ready result alone does not establish that a new conversion
was triggered. Conversion scheduling, polling intervals, stale-sample handling,
and deadlines belong to the application. Bus operation timeouts belong to the
bus implementation or caller.

The initial scope covers measurement. ALERT/SMBus alert response, threshold
register access, and general-call reset are not implemented. Changing extended
mode also changes the interpretation of existing threshold registers; this
driver does not rewrite them.

## Errors, cancellation and recovery

`Error::I2c` preserves the underlying bus error, including an address NACK for an
absent sensor. The driver does not hide faults, retry transactions, or attempt
bus recovery. `bus_mut()` and `into_inner()` give the caller access to recovery
provided by its bus implementation.

There is no cached device configuration. Cancelling a bus future inherits that
bus's cancellation semantics. A cancelled or failed write may have partially
changed configuration or started a conversion; recover the bus if needed, read
back device state and reapply settings. Configuration uses a read-modify-write
across separate transactions: do not configure the same sensor concurrently
through multiple handles.

## Minimal Embassy example

[`examples/embassy.rs`](examples/embassy.rs) is a standalone Pico/Pico 2
usage example with all setup in one file.

Reads a TMP102/TMP112 once per second and logs milli-Celsius over RTT.
Connect SDA to GP6, SCL to GP7, ADD0 to ground (0x48), supply to 3.3 V,
and ground to the Pico ground. Use external I²C pull-ups to 3.3 V.
The first read waits one second for an initial conversion.

Build from the workspace root (choose one board):

```sh
# Pico / RP2040
cargo build -p polis-tmp1x2 --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release --locked
# Pico 2 / RP2350A
cargo build -p polis-tmp1x2 --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

The ELF is `.target/<target>/release/examples/embassy`. Flash the matching
image with your usual SWD/UF2 workflow; an RTT-capable debug probe displays
`defmt` output. These commands build and link only; they do not flash hardware.
The opt-in features supply the executor, time driver, panic logger, boot image,
and package-local Pico/Pico 2 memory layout. Normal library builds keep their
existing runtime and MCU choices. Enable only one example board feature.
Examples stop on I/O errors with `unwrap()` to keep the usage path short;
applications own deadlines and recovery. Builds do not qualify hardware.

## Validation

Run from the `polis-rs/` root:

```sh
cargo test -p polis-tmp1x2
cargo build -p polis-tmp1x2 --target thumbv6m-none-eabi --release
```

Host tests use a bus implementing only the standard async I²C trait. Its
transactions suspend before completion. Fixtures cover signed normal/extended
samples, address selection, exact transaction shapes, configuration bit masks,
one-shot busy/ready transitions, transport errors, partial-write state and
cancellation before a configuration write starts.

Builds and host tests do not qualify physical hardware. Sensor response, one-shot
conversions, hot-plug/recovery and electrical timing require bench evidence.

## Source and distribution

See [PROVENANCE.md](PROVENANCE.md) for source hashes, extraction changes and
license status.

## License

Licensed under MIT; see [LICENSE-MIT](LICENSE-MIT).
