# Initial library validation

Validated on 2026-10-11 with Rust 1.99.0, after adopting bare driver package names.

## Repository checks

| Check | Result |
| --- | --- |
| `python3 tools/check.py test` | 70 unit, integration and documentation tests passed |
| `python3 tools/check.py build --platform all` | All ten libraries built individually on native, RP2040 and RP2350; WS2813 example linked on both MCUs |
| `python3 tools/check.py clippy --platform all` | Passed with warnings denied for local packages |
| `RUSTDOCFLAGS='-D warnings' python3 tools/check.py doc --platform all` | API documentation built for all three configurations |
| `cargo package --workspace` | All ten archives created and verified; dependency paths normalized and source notices/provenance included |

Formatting and local Markdown links also passed review. Archive inspection checked
that each package contains its README/provenance and that normalized dependencies
do not retain local workspace paths. Archives with declared licenses contain the
corresponding notice files.

The offline workspace packaging attempt encountered a Cargo temporary-registry
checksum error. Normal registry-enabled packaging passed. Cargo also reports a
future-compatibility warning in the upstream `proc-macro-error2` dependency; this
did not fail the current compiler/build matrix.

## Consumer checks

Temporary copies under `/tmp/polis-consumer-validation` retained the original
consumer source and aliased its existing dependency names to these shared packages.
Original product checkouts were not edited. The temporary Slate copy used the
validated Rust 1.99.0 compiler rather than its original 1.95.0 pin.

| Consumer | Result |
| --- | --- |
| Slate `test-rs485`, `test-tmp1x2` with PIO I2C, `test-sk6812`, `test-ws2813` | All four linked separately for RP2040 and RP2350: eight images |
| Slate controller BSP `link_check` | RP2350 example linked with shared W5500/UART/RS-485 libraries |
| Slate Mini and Large firmware | Both RP2040 images linked in separate invocations with shared output and driver libraries |
| Wired-control protocol | 20 tests passed against shared framing |
| Wired-control firmware | RP2350 firmware linked against shared framing |

Each Slate bench build used `--no-default-features`, its matching `rp2040` or
`rp235xa` feature, target triple and `--profile firmware`; the TMP1x2 bench also
selected `pio-i2c`. Mini and Large require separate builds because their BSP
features are mutually exclusive. Consumer aliases preserve source imports while
selecting the new upstream package identity.

## Remaining release and adoption work

No hardware was flashed or electrically qualified. Discovery provides the shared
wire contract and application filtering, not live socket/session implementations.
Existing discovery consumers still need to adopt the common schema explicitly.

Original packages now use the owner-selected MIT license. Existing MIT/BSD
notices remain on the imported PIO packages. Establish the actual repository URL
and registry ownership
before publication; resolve name conflicts at that time. Remote CI is configured
but has not run because this directory has no established Git remote.

## LED macro validation

After adding `led_outputs!`, the focused package suite passed eight unit tests
and four doctests, including three compile-fail cases. It covers evaluation once
in argument order, caller-name hygiene, mixed lane lengths, eight lanes,
concurrent completion, bad frame lengths and cancellation.

```sh
cargo test --locked -p led-output
python3 tools/check-led-macro.py
python3 tools/check.py build --platform all --package led-output --package pio-sk6812 --package pio-ws2813
python3 tools/check.py clippy --platform all --package led-output --package pio-sk6812 --package pio-ws2813
cargo fmt --all -- --check
```

All commands passed. The isolated caller imports the dependency as `led-api`,
uses borrowed, const-generic lanes without `std`, and builds on the host and both
MCUs. The WS2813 example now uses the macro and links on both MCUs. Separate
Mini/Large firmware builds in the temporary Slate consumer copy also passed.

## ADS1x1x extraction validation

Validated on 2026-10-11 with Rust 1.99.0. The workspace now has eleven libraries.
The new `ads1x1x` package has 12 unit tests and three doctests (including a
compile-fail check for selecting a 12-bit rate on a 16-bit model). The complete
workspace suite passes 91 tests.

| Check | Result |
| --- | --- |
| `cargo test --workspace --locked --offline` | 91 unit, integration and documentation tests passed |
| `python3 tools/check.py build --platform all --package ads1x1x` | Native, RP2040 and RP2350 library builds passed |
| `python3 tools/check.py clippy --platform all --package ads1x1x` | All three configurations passed with warnings denied |
| `RUSTDOCFLAGS='-D warnings' cargo doc -p ads1x1x --no-deps --locked --offline` | Passed; README examples are also compiled by doctests |
| `cargo package -p ads1x1x --allow-dirty --offline` | Standalone archive created and verified; source, tests, README, changelog and provenance included |

Wire fixtures exercise both blocking and yielding async I²C implementations:
all six models, four addresses, signed 12/16-bit samples, both rate tables,
thresholds, ready-output setup, model capability rejection, trigger/MUX writes,
bounded polling, transport failures and reconfiguration after cancellation.
Formatting also passes `cargo fmt --all -- --check`.

An isolated consumer at `/tmp/ads1x1x-validation` linked release ELF images for
both MCUs using Embassy RP 0.10.0, Embassy Time 0.5.1 and Executor 0.10.0. It
instantiates all six models with actual blocking and async Embassy I²C types,
configures them, performs bounded reads and scales samples. Its memory maps and
linker setup follow the existing WS2813 link fixture. Reproduction in this
session:

```sh
cargo build --offline --locked --manifest-path /tmp/ads1x1x-validation/Cargo.toml --release --target thumbv6m-none-eabi --features rp2040
cargo build --offline --locked --manifest-path /tmp/ads1x1x-validation/Cargo.toml --release --target thumbv8m.main-none-eabihf --features rp235xa
```

The temporary consumer is a link check, not a product migration or hardware
qualification. No ADC was exercised electrically. The original UTSuites driver
checkout remains clean. MIT was subsequently selected by the owner. Public
repository metadata is still needed before publication; no registry upload was
attempted.
