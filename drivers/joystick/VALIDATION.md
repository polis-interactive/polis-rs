# Validation record

Date: 2026-10-19. Compiler: Rust 1.99.0. Embassy RP: 0.10.0.
Hardware qualification: **not performed**. No board was flashed or measured.

## Software evidence

| Check | Result |
| --- | --- |
| Core host tests | 3 passed |
| Core plus ADS adapter tests | 6 passed, including all four retained original tests |
| Native, RP2040 and RP2350A library build matrix | Passed; independent and combined adapters |
| Upstream Pico/Pico 2 example release links | Passed |
| Upstream matrix Clippy, `-D warnings` | Passed, including examples |
| Upstream matrix API documentation / rustfmt | Passed |
| HDMI protocol, BSP and input mirror host tests | 7 passed |
| Affected HDMI host and both RP2040 consumer Clippy configurations | Passed with `-D warnings` |
| Migrated `hdmi-control-surface` firmware link | Passed before retiring old local package |
| Migrated `onchip-joystick` example link | Passed before retiring old local package |
| Local ADS + joystick package archives and Cargo verification | Passed offline; no publication |
| ADS-enabled package verification through Cargo's temporary registry | Passed with registry access |
| Extracted archives outside the workspace | 6 host tests passed; Pico/Pico 2 examples linked |

The shared-ADC test uses two distinct Joystick objects and successive mutable
borrows of one ADS1015, checks X/Y channel order and preservation of the ADC
range, and retains native signed counts. Error tests cover first-axis early
exit and second-axis rejection of the partial pair. Exact asymmetric integer
truncation and extreme `i16` count inputs have explicit assertions.

Runtime source was transferred without arithmetic or acquisition changes.
The firmware and on-chip example only change their joystick import/dependency;
BSP wiring, product calibration, timing/recovery and control-link payload types
remain downstream. Cargo's reverse dependency tree shows one local
`polis-ads1x1x` shared by the firmware, BSP and joystick adapter.

## Reproduce

From polis-rs:

```sh
python3 tools/check.py test --package polis-joystick
python3 tools/check.py build --platform all --package polis-joystick
python3 tools/check.py clippy --platform all --package polis-joystick
python3 tools/check.py doc --platform all --package polis-joystick
cargo fmt --all -- --check
cargo package --locked --offline --allow-dirty -p polis-ads1x1x -p polis-joystick
cargo package --locked --allow-dirty -p polis-ads1x1x -p polis-joystick --features polis-joystick/ads1x1x
```

From the sibling HDMI checkout, with its documented local validation patches:

```sh
cargo test --locked --offline -p hdmi-control-protocol -p hdmi-control-surface-bsp -p hdmi-input-pico
cargo build --locked --offline -p hdmi-control-surface --target thumbv6m-none-eabi --profile firmware
cargo build --locked --offline -p hdmi-control-surface --example onchip-joystick --features onchip-joystick-example --target thumbv6m-none-eabi --profile firmware
cargo clippy --locked --offline -p hdmi-control-protocol -p hdmi-control-surface-bsp -p hdmi-input-pico --all-targets -- -D warnings
cargo clippy --locked --offline -p hdmi-control-surface --bin hdmi-control-surface --target thumbv6m-none-eabi --profile firmware -- -D warnings
cargo clippy --locked --offline -p hdmi-control-surface --example onchip-joystick --features onchip-joystick-example --target thumbv6m-none-eabi --profile firmware -- -D warnings
```

Both workspaces use their own root `.target/`, verified with Cargo metadata.
Standalone archive commands must set an absolute `CARGO_TARGET_DIR` pointing
to the owning repository's `.target/`; no package-local `target/` is intended.
Tokio is a host-only dev dependency and does not enter the embedded example graph.

ADS-enabled offline archive verification initially hit Cargo 1.99's known
temporary-registry error, `no hash listed for polis-ads1x1x v0.1.0`. The same
verification with registry access passed; it does not upload or publish packages.
Both archives were also extracted under `/tmp`. The joystick archive used a
temporary `[patch.crates-io]` pointing to the **extracted ADS archive**, not the
source checkout, for its six host tests and both linked examples. All those
commands set `CARGO_TARGET_DIR=/home/broosegoose/Polis/polis-rs/.target` explicitly.
The examples linked with ordinary Cargo release defaults outside workspace
configuration/profiles.

## Limits and release prerequisites

The original source package and example were untracked; the recorded source
checkout revision does not contain them. The extracted package is still
uncommitted/unpublished. HDMI therefore uses temporary local Cargo patches;
replace these with real release/commit coordinates and validate again before
committing the downstream migration. No commit, push or publication occurred.

`proc-macro-error2 2.0.1` produces an existing future-compatibility warning.
The existing ADS package lacks repository/homepage metadata and Cargo warns
during packaging; its implementation and manifest were left unchanged.

Tests and links do not establish physical wiring, count accuracy, calibration,
ADC noise, sampling skew, deadlines, or cancellation recovery on real hardware.
Those remain application/hardware qualification work.
