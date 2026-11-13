# Validation

Validated on 2026-11-13 with Rust 1.99.0. All Cargo artifacts, including builds
from an archive extracted outside the workspace, use the repository-root `.target/`.

| Check | Result |
| --- | --- |
| Host unit tests | 15 passed |
| Rust documentation example | Compiled successfully |
| Native library/test build | Passed |
| RP2040 library build | Passed, `thumbv6m-none-eabi` |
| RP2350 library build | Passed, `thumbv8m.main-none-eabihf` |
| Embassy example | Release ELFs linked for both targets |
| Clippy | Passed with `-D warnings` for host and both embedded libraries/examples |
| API documentation | Built successfully |
| Formatting/static review | Workspace rustfmt, whitespace, local documentation links and package metadata reviewed |
| Package verification | Offline `cargo package --allow-dirty` built and verified the archive |
| Independent archive | Extracted under `/private/tmp`; all 15 tests and doctest passed; both RP examples linked without workspace files |

Tests use an async mock with suspension before and after each simulated hardware
effect. Coverage includes both address banks and all straps, invalid pin/address
rejection, register-pointer/repeated-START boundaries, whole-port writes, every
pin and mask, initialization order, output-latch versus physical-input semantics,
readback after simulated reset, original errors without retries, and cancellation
at each initialization/update transaction stage. Hardware is not rolled back
after failed or cancelled writes; tests verify explicit subsequent operations
read fresh state or reapply the full initialization sequence.

Commands run from the repository root:

```sh
cargo test --locked --offline -p polis-xca953xa
python3.14 tools/check.py build --platform all --package polis-xca953xa
python3.14 tools/check.py clippy --platform all --package polis-xca953xa
cargo doc --locked --offline -p polis-xca953xa --no-deps
cargo fmt --all -- --check
git diff --check
cargo package --offline --allow-dirty -p polis-xca953xa
```

The default `python3` lacked `tomllib`; the installed Python 3.14 ran the matrix.
The first example-build attempt needed uncached `zerocopy` dependencies and hit
sandbox DNS restrictions. Fetching the locked dependencies with network access
resolved this; the complete build matrix then passed. Cargo reports the existing
future-compatibility warning for transitive `proc-macro-error2` 2.0.1.

For independent archive checks, `CARGO_TARGET_DIR` was explicitly set to
`/Users/brucewilcoxon/Polis/polis-rs/.target` and `cargo +1.99.0` selected the tested
compiler. The archive includes source/tests, example, build script, both memory
maps, documentation and license. No package was published.

## Controller panel adaptation

The example now uses I²C0 GP8/GP9 and the Controller's five active-low button
inputs and three LED sinks. Both target examples were rebuilt/linked and passed
Clippy after this change. The driver implementation is unchanged.
Static review checked pin assignments against the schematic generator and
`gpio.json`, and LED behavior against the controller design notes.

On hardware, press CCW, STOP and CW individually: the matching LED should light
and its direction configuration should become 0xF7, 0xDF and 0xEF respectively.
With all three pressed the configuration should be 0xC7; releasing all three
should restore 0xFF. MINUS/PLUS should change the logs without lighting an LED.
Output and polarity registers should remain zero throughout.

## Remaining hardware validation

No board was flashed or electrically qualified. Run the wired example from the
README on each intended part/board. Verify address straps, input polarity, output
latch readback, output release during initialization, and state after power cycling.
Use a logic analyzer to confirm I²C transactions and INT acknowledgement, and
exercise the selected HAL's timeout/cancellation recovery. Mock tests and linked
ELFs do not establish bus recovery, INT edge capture or glitch-free board behavior.
