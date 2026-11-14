# polis-tmc5160

Allocation-free, `no_std`, async SPI configuration and diagnostics for TMC5160/A
with an external STEP/DIR source. Uses `embedded-hal-async` 1.0 and selects no
MCU, executor, pins or motor current in normal library builds.

## Scope

- Typed registers for global/DIAG configuration, current scaling, chopper and
  microstep settings, StealthChop, CoolStep/StallGuard, bridge protection and
  status. Reserved bits and unsupported field encodings are rejected.
- An explicit `BoardConfig::BTT_TMC5160T_PLUS` preset for BTT's **V1.0** module:
  22 mΩ sense resistors and the published 10.6 A RMS board ceiling. There is no
  default board. Custom boards use `BoardConfig::new(resistance_microohms,
  maximum_rms_milliamps)`.
- STEP/DIR generation, motion planning, ramp-generator commands, UART, OTP
  writes, EN/brake sequencing and product fault policy are outside the API.
  A future motion wrapper can compose this driver without changing SPI ownership.

## Configuration

```rust,no_run
use embedded_hal_async::{delay::DelayNs, spi::SpiDevice};
use polis_tmc5160::{BoardConfig, Error, StepDirConfig, Timing, Tmc5160};
use polis_tmc5160::registers::{Chopconf, DrvStatus, Microsteps};

async fn configure<S: SpiDevice<u8>, D: DelayNs>(
    spi: S,
    delay: D,
    run_rms_ma: u32,
    hold_rms_ma: u32,
) -> Result<Tmc5160<S, D>, Error<S::Error>> {
    let board = BoardConfig::BTT_TMC5160T_PLUS;
    let current = board.current_settings(run_rms_ma, hold_rms_ma, 4)
        .map_err(Error::Config)?;
    // Inspect current.run_rms_milliamps()/hold_rms_milliamps() for quantization.
    // Example chopper starting point: tune for your motor, supply and load.
    let chopper = Chopconf::DISABLED
        .with_microsteps(Microsteps::M16, true)
        .with_off_time(3).map_err(Error::Config)?;
    let config = StepDirConfig::new(current, chopper);
    let mut driver = Tmc5160::new(spi, delay, board, Timing::INTERNAL_CLOCK);
    // Caller already holds DRV_ENN high and STEP inactive, and keeps them so.
    driver.configure(&config).await?;
    let diagnostics = driver.read::<DrvStatus>().await?;
    // Inspect diagnostics and perform product-owned enable/pulse sequencing later.
    Ok(driver)
}
```

`configure` validates all values before I/O, checks IOIN version 0x30, external
STEP/DIR mode and asserted DRV_ENN, and writes CHOPCONF with TOFF=0 first. It
then applies current and other settings, finishing with the requested CHOPCONF.
It does not drive ENN or STEP. **The caller must keep ENN high for the entire
operation**, including recovery. The final CHOPCONF can arm the bridges for a
later external enable. `Chopconf::DISABLED` keeps TOFF zero when that is desired.

`StepDirConfig::new` selects SpreadCycle, disables CoolStep and velocity
thresholds, and uses datasheet PWM settings. Selecting StealthChop requires
appropriate motor-specific tuning, including externally supplied motion during
auto-tuning. `short` and `bridge` are optional complete register values: `None`
preserves the chip's existing/OTP settings. The BTT documents do not establish
tuned replacements for these values, so the preset does not invent them.

Low-level `read::<R>()` and `write(value)` use register types from `registers`.
Write-only registers cannot be read through this API. Complete writes avoid
hidden shadow registers and read-modify-write against unreadable hardware.
`from_bits` supports detailed register configuration; helper methods cover
common microstep, current, mode and StallGuard choices. Direct/test modes in
GCONF are excluded. GSTAT is acknowledged separately with `clear_status`, so
reading diagnostics never implicitly acknowledges faults. Raw typed writes
validate register encodings but do **not** enforce motor/board current limits or
external pin state; use the checked configuration path for initialization.

## SPI ownership and timing

Supply an async `SpiDevice<u8>` configured for **mode 3, MSB-first**. The driver
sends one five-byte transfer per transaction. Reads issue two distinct CS
transactions to the same address, discarding the first data response. Both
returned status bytes are exposed. A write's returned status describes the chip
at the start of that transfer; neither it nor a successful SPI transfer proves
the register was accepted by the chip. Use readback for readable registers when
needed; write-only values cannot be verified this way.

The driver delays with CS high **before every datagram**, including successive
public API calls. `Timing::INTERNAL_CLOCK` uses 1 µs; custom clocks must satisfy
the datasheet's CS-high requirement (>2*tCLK + 10 ns). The backend must flush
the final bit before releasing CS and satisfy the SCK/CS setup/hold requirements.
`Operation::DelayNs` occurs with CS asserted and cannot replace the inter-frame
delay. Choose the SPI frequency for the chip clock, wiring and signal integrity;
both supplied examples use 1 MHz with the internal clock.

One logical owner must control each chip's pipelined reads. A shared SPI bus with
other chip-select lines is fine, but multiple driver instances for the same
chip are not. Embassy RP hardware SPI and PIO SPI both implement async `SpiBus`;
`embedded_hal_bus::spi::ExclusiveDevice` adds CS to either. A shared-bus
`SpiDevice` adapter can be supplied instead.

## Errors, cancellation and resets

Every underlying SPI error is preserved. Any failed or cancelled I/O operation
blocks subsequent I/O with `RecoveryRequired`, including cancellation while
waiting for the CS-high interval. There are no automatic retries. The driver
cannot itself stop DMA or release a CS pin owned by a generic `SpiDevice`.

For a backend with a recovery API, `recover_with(async |spi| { ... })` must stop
or drain pending transfers, restore CS high and leave the peripheral idle.
Successful recovery also waits the configured CS-high interval. Failed or
cancelled recovery remains blocked. If the backend cannot be repaired in place,
release/rebuild it using `into_parts` and construct a new driver only after the
physical bus is idle. `into_parts` does not repair the backend or stop the motor.

In particular, `embedded-hal-bus` 0.3.0's `ExclusiveDevice` releases CS on normal
completion/error, but does not provide CS cleanup when a future is dropped.
The supplied examples await transfers to completion and stop on errors; they
do not demonstrate recovery or timeout cancellation of that adapter.

`configuration_progress` reports precondition checks or the count of completed
writes and address of the uncertain write after an error/cancellation. Complete
means the transfers completed, not that the motor is qualified or that settings
survived a reset. Do not resume a partially completed sequence by its count:
recover, inspect status and explicitly reapply the full configuration as needed.
Observe reset flags and reconfigure after power loss. No cached register values
are presented as current hardware state.

## BTT current and integration notes

The preset follows the BTT TMC5160T Plus V1.0 manual and schematic R5/R15, both
22 mΩ. Current calculation uses the TMC5160 formula with typical VSRT = 325 mV:

```text
I_RMS = (GLOBALSCALER / 256) * ((CS + 1) / 32) * 0.325 / (R_SENSE * sqrt(2))
```

GLOBALSCALER zero encodes 256; values 1..31 are rejected. The helper chooses the
largest representable IRUN, rounds GLOBALSCALER down, and then rounds IHOLD down.
It returns nominal RMS milliamps after quantization. Actual current depends on
sense/IC tolerances, operating mode and tuning. Requests above the board ceiling
or the nominal calculable range fail; they are not silently clamped. In
particular, 325 mV gives about **10.446 A** full scale with 22 mΩ, while BTT's
published rounded ceiling is 10.6 A. Zero-current requests are rejected because
CS=0 means 1/32 scale, not disabled outputs. Use ENN/TOFF for disable. Very low
run/hold ratios may not be representable. Low current settings do not establish
that StealthChop automatic tuning requirements have been met.

The board rating is not a selected motor current. BTT specifies active cooling
above 3 A and driver-power-before-logic-power sequencing. Follow the board manual
for supplies/adapters and select SPI plus external STEP/DIR using its hardware
straps. Product firmware still owns motor limits, supply-dependent protection
tuning, cooling, enable/brake behavior and diagnostic decisions.

## Embassy examples and validation

Both examples only read identity/status and keep EN high, STEP low and DIR low.
They neither configure current nor energize the motor. Use the BTT logic
interface with appropriate logic levels and common ground; motor power is a
separate supply. Connections: GP18 SCK, GP19 MOSI, GP16 MISO, GP17 CS, GP20 EN,
GP21 STEP, GP22 DIR. IOIN must report version 0x30, SD_MODE high and DRV_ENN high.
RTT prints GSTAT, DRV_STATUS and the SPI response status once per second.
`embassy` uses SPI0; `embassy_pio` uses PIO0 SM0. Both use DMA_CH0/1.

```sh
cargo test -p polis-tmc5160 --locked
python3 tools/check.py build --platform all --package polis-tmc5160
python3 tools/check.py clippy --platform all --package polis-tmc5160
cargo build -p polis-tmc5160 --release --target thumbv6m-none-eabi --example embassy --features example-rp2040
cargo build -p polis-tmc5160 --release --target thumbv8m.main-none-eabihf --example embassy_pio --features example-rp235xa
```

The matrix requires Python 3.11+ and links both examples on both chips. All Cargo
artifacts use the repository root `.target/`. Host tests cover byte order, status,
CS transaction boundaries/delay, read pipelining, register/current validation,
partial configuration, and errors/cancellation across send/recovery awaits.

Hardware qualification remains outstanding: verify SPI edges/CS timing and
backend recovery with a logic analyzer, check module straps and identity, measure
actual phase currents, and tune/measure thermal, short-detection and chopper
behavior on the selected supply and motor. Builds and mock tests do not replace
these checks. No hardware has been flashed or downstream firmware migrated.
