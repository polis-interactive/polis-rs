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
MCUs. The WS2813 example now uses the macro and links on both MCUs.

## ADS1x1x async validation

Validated on 2026-10-11 with Rust 1.99.0. The workspace now has eleven libraries.
After removing the blocking API, `ads1x1x` has 12 unit tests and two doctests (including a
compile-fail check for selecting a 12-bit rate on a 16-bit model). The complete
workspace suite passes 90 tests.

| Check | Result |
| --- | --- |
| `cargo test --workspace --locked --offline` | 90 unit, integration and documentation tests passed |
| `python3 tools/check.py build --platform all --package ads1x1x` | Native, RP2040 and RP2350 library builds passed |
| `python3 tools/check.py clippy --platform all --package ads1x1x` | All three configurations passed with warnings denied |
| `RUSTDOCFLAGS='-D warnings' cargo doc -p ads1x1x --no-deps --locked --offline` | Passed; README examples are also compiled by doctests |
| `cargo package -p ads1x1x --allow-dirty --offline` | Standalone archive created and verified; source, tests, README, changelog and provenance included |

Wire fixtures exercise yielding async I²C transactions:
all six models, four addresses, signed 12/16-bit samples, both rate tables,
thresholds, ready-output setup, model capability rejection, trigger/MUX writes,
bounded polling, transport failures and reconfiguration after cancellation.
Formatting also passes `cargo fmt --all -- --check`.

An isolated consumer at `/tmp/ads1x1x-validation` linked release ELF images for
both MCUs using Embassy RP 0.10.0, Embassy Time 0.5.1 and Executor 0.10.0. It
instantiates all six models with an actual async Embassy I²C bus,
configures them, performs bounded reads and scales samples. Its memory maps and
linker setup follow the existing WS2813 link fixture. Reproduction in this
session:

```sh
cargo build --offline --locked --manifest-path /tmp/ads1x1x-validation/Cargo.toml --release --target thumbv6m-none-eabi --features rp2040
cargo build --offline --locked --manifest-path /tmp/ads1x1x-validation/Cargo.toml --release --target thumbv8m.main-none-eabihf --features rp235xa
```

The temporary consumer is a link check, not a product migration or hardware
qualification. No ADC was exercised electrically. MIT was subsequently selected
by the owner. Public repository metadata is still needed before publication;
no registry upload was attempted.

## Minimal Embassy driver examples

Validated on 2026-10-11 with Rust 1.99.0. All eight drivers now include
`examples/embassy.rs`, with package-local runtime features and Pico/Pico 2
linker inputs. Source usage demos were simplified; no hardware was flashed.

| Check | Result |
| --- | --- |
| `python3 tools/check.py build --platform all` restricted to the eight driver packages | Native and both MCU library builds passed; all eight Embassy examples linked on both MCUs, plus the existing WS2813 `link_check` |
| `python3 tools/check.py clippy --platform all` with the same package selection | Libraries and embedded examples passed with warnings denied |
| `cargo test --workspace --locked` | 90 unit, integration, and documentation tests passed |
| `cargo fmt --all -- --check` and `git diff --check` | Passed |
| `cargo package -p <driver> --list --allow-dirty --locked` for each driver | All eight package file lists include the usage example, build script, both memory maps, README, and provenance |

The matrix selection used repeated `--package` arguments for `ads1x1x`,
`tmp1x2`, `rs485`, `w5500`, `pio-i2c`, `pio-uart`, `pio-sk6812`, and
`pio-ws2813`. Individual example commands and wiring are in each README.
Archive checks here inspect contents; they do not claim standalone archive
build verification. The existing upstream `proc-macro-error2` future-compatibility
notice remains. Physical sensor readings, serial timing, LED waveforms, and
Ethernet traffic still require bench validation.
