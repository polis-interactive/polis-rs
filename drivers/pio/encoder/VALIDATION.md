# Validation record

Date: 2026-10-19. Compiler: repository-pinned Rust 1.99.0.
Dependency lock: embassy-rp 0.10.0, pio 0.3.0, fixed 1.31.0.

Hardware qualification: **not performed**. No board was flashed or measured.
The downstream HDMI project was not modified or built.

## Completed checks

| Check | Result |
| --- | --- |
| Focused portable and emitted-PIO instruction tests | 14 passed; includes 16,384 exhaustive traces |
| Native / RP2040 / RP2350A library builds | Passed |
| Three-encoder Pico and Pico 2 release example links | Passed |
| Native / both targets Clippy, including examples, `-D warnings` | Passed |
| Native / both target API documentation | Passed |
| Workspace rustfmt and `git diff --check` | Passed |
| `cargo package --locked --offline --allow-dirty` | Passed, including verification; 20-file archive inspected |
| Extracted archive outside workspace: both release example links | Passed with ordinary Cargo release defaults |

## Reproduction

```sh
cargo test --locked -p polis-pio-encoder
python3 tools/check.py build --platform all --package polis-pio-encoder
python3 tools/check.py clippy --platform all --package polis-pio-encoder
python3 tools/check.py doc --platform all --package polis-pio-encoder
cargo fmt --all -- --check
cargo package --locked --offline --allow-dirty -p polis-pio-encoder
cargo package --locked --offline --allow-dirty -p polis-pio-encoder --list
```

The archive was extracted under `/tmp` and linked with Rust 1.99.0 using these
commands from the extracted package directory (CARGO_TARGET_DIR pointed to a
build cache only; no workspace Cargo config or profiles were inherited):

```sh
cargo +1.99.0 build --locked --offline --release --example embassy --features example-rp2040 --target thumbv6m-none-eabi
cargo +1.99.0 build --locked --offline --release --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf
```

Package code, tests, licenses, memory maps and build script are self-contained.
The registry-connected `cargo publish --dry-run` was not run: offline mode
requires HTTP, and automatic approval review rejected network execution as
possible source export to crates.io. Local archive verification and independent
target links passed. Nothing was published. Fresh online dependency resolution
and registry publication remain release-time checks.

Cargo reports an existing dependency future-incompatibility warning for
`proc-macro-error2 2.0.1`; package Clippy passes with warnings denied.

## Outstanding hardware evidence

Before qualifying a particular board/encoder, measure both inputs at maximum
rotation/bounce rates, verify minimum intermediate-phase dwell against the chosen
clock divider, confirm physical direction and transitions per detent, and test
long transport outages while the application retains its tracker. Check startup
with every settled phase and board-level pull/RC behavior. No software check here
establishes any of those measurements or exact missed-motion detection.
