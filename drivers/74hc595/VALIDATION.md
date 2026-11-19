# Validation

Validated on 2026-11-18 with Rust 1.99.0 on aarch64-apple-darwin.
All Cargo artifacts use the repository-root `.target/`.
The system `python3` lacks `tomllib`; the matrix runs use installed Python 3.14.

## Automated checks

| Check | Result |
| --- | --- |
| `cargo test -p polis-74hc595 --offline` | Six behavioral tests and one compiled doctest passed |
| `python3.14 tools/check.py build --platform all --package polis-74hc595` | Host and both Arm library builds passed; Pico/Pico 2 release examples linked |
| `python3.14 tools/check.py clippy --platform all --package polis-74hc595` | Host tests/library and both Arm library/example configurations passed with `-D warnings` |
| `RUSTDOCFLAGS='-D warnings' cargo doc -p polis-74hc595 --locked --no-deps` | API docs passed |
| `cargo fmt -p polis-74hc595 -- --check` and `git diff --check` | Passed |

Host tests model individual shift-clock edges, physical completion delayed until
flush, and storage updates only on rising STCP. They cover all 256 single-chip
values, distinct bytes across a three-chip chain, clear/repeated writes,
nonzero chain validation, borrowed resources, and resource recovery. Every
fallible operation is injected with errors both before and after its potential
hardware effect. Every async suspension is cancelled before and after its effect;
a subsequent frame must restore the output state and honor the latch high time.

The example build emits an existing dependency future-incompatibility notice
for `proc-macro-error2 2.0.1`. It does not prevent linking or current Clippy checks.

## Archive verification

The initial rotating-output example passed package listing and offline archive
verification. That archive was also extracted under `/private/tmp` and both release examples linked there with
Rust 1.99.0 and an explicit repository-root `CARGO_TARGET_DIR`. These builds
used the normalized manifest without the workspace profiles or Cargo config.

Reproduce package inspection and verification with:

```sh
cargo package -p polis-74hc595 --allow-dirty --list
CARGO_TARGET_DIR="$PWD/.target" cargo package -p polis-74hc595 --allow-dirty --offline
```

`--allow-dirty` includes the requested uncommitted work; it does not publish.
The archive includes its own license, README, tests, example, memory maps and
build script. For builds from an extracted archive outside the workspace,
set `CARGO_TARGET_DIR` to this repository's absolute `.target/` path explicitly.
