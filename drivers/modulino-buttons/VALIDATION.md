# Validation

Validated on 2026-10-21 with Rust 1.99.0. All build artifacts use the
repository-root `.target/` directory.

| Check | Result |
| --- | --- |
| Host unit tests | 9 passed; exact protocol bytes, addresses, identifier validation and transport errors |
| Rust documentation example | Compiled successfully |
| Native library and test build | Passed |
| RP2040 library build | Passed for `thumbv6m-none-eabi` |
| RP2350 library build | Passed for `thumbv8m.main-none-eabihf` |
| Embassy example, RP2040 | Release ELF linked successfully with `example-rp2040` |
| Embassy example, RP2350 | Release ELF linked successfully with `example-rp235xa` |
| Clippy | Passed with `-D warnings` for native and both embedded libraries/examples |
| API documentation | Built successfully with `cargo doc --locked --no-deps` |
| Formatting and whitespace | Workspace rustfmt check and `git diff --check` passed |
| Standalone package | `cargo package --offline --allow-dirty` built and verified the archive; source, tests, example, memory maps and license are included |

Commands:

```sh
cargo test --offline -p polis-modulino-buttons
python3 tools/check.py build --platform all --package polis-modulino-buttons
python3 tools/check.py clippy --platform all --package polis-modulino-buttons
cargo doc --locked -p polis-modulino-buttons --no-deps
cargo package --offline --allow-dirty -p polis-modulino-buttons
```

Cargo reports an existing future-compatibility warning for the example's
transitive dependency `proc-macro-error2` 2.0.1. It does not prevent these builds.

No ABX00110 hardware was flashed or exercised. Mock tests establish host protocol
behavior; compiled libraries and linked examples establish target/toolchain
integration. Neither establishes electrical operation, firmware timing or
debounce behavior on a real board. To qualify hardware, run the wired example,
verify each button and simultaneous combinations, and confirm the matching LEDs
and logged levels. Custom-address operation also needs a reconfigured module.
