# Validation — 2026-11-19

## Host behavior

`cargo test --locked -p polis-settings`: 18 unit tests and one README doctest pass
on the repository-pinned Rust 1.99.0, native aarch64 macOS host.

Tests use the real sequential-storage 8.0.2 map and an original NOR flash model.
For a representative append and a representative page migration (which must
include erase), each read boundary and each programmed/erased byte boundary is
interrupted in turn. Error cases reconstruct the store; cancellation cases drop
a pending future and reuse the same store. Each case must recover the complete
old or new record, retain another live key, remain writable and stay within guard
regions. No wrapper erase-on-error is used. The model uses 128-byte erase pages
and includes prefix-torn writes/erases; it does not exhaust every physical flash
failure pattern, voltage behavior, or interrupted-recovery sequence.

Additional tests cover bit corruption across a stored item's header/key/value,
storage exhaustion and read/reset failures, explicit reset cancellation/retry,
unsupported envelope/schema versions, malformed/trailing payloads, explicit
migration, preservation of larger unknown records through page migration,
configuration rejection without I/O, wire bytes, maximum/oversized records and
the consumer's generation comparison while a save is pending. Geometry tests
exercise read/write sizes 4/1 (RP async), 1/4, 4/4 and 32/32.

CRC-damaged records may appear missing because the backend skips them. Tests do
not claim every physical corruption produces a visible backend error.

## Embedded integration

Library compilation and Pico/Pico 2 example linking have passed for
`thumbv6m-none-eabi` and `thumbv8m.main-none-eabihf`. Examples use the actual
Embassy RP 0.10.0 async flash implementation, four-byte aligned scratch, and
linker symbols for the reserved final 16 KiB. Host and target matrix commands
use `python3.14` here because the system `python3` lacks Python 3.11's `tomllib`.

The final native/RP2040/RP2350 build and Clippy matrices pass, including both
linked examples. Clippy runs with `-D warnings`. Cargo also reports an existing
dependency future-incompatibility advisory for `proc-macro-error2` 2.0.1;
it does not fail these checks.

`cargo fmt -p polis-settings -- --check`, documentation with
`RUSTDOCFLAGS='-D warnings'`, and `git diff --check` pass. The standalone package
archive was listed and verified with `cargo package --locked --offline
--allow-dirty -p polis-settings`, with `CARGO_TARGET_DIR` explicitly set to the
repository's absolute `.target/` path. The archive includes both linker layouts,
the example, license, source/tests and package documentation; no sibling checkout
is required. No dependency versions changed except adding the pinned backend.

ELF inspection confirms all flash load segments end before the settings symbol:
RP2040 reserves `0x101fc000..0x10200000`; RP2350 reserves
`0x103fc000..0x10400000`. Runtime code uses those same exported symbols.

## Remaining qualification

No board was flashed and no downstream HC firmware was modified or linked.
Product adoption still requires its own linker/runtime consistency check,
power-interruption tests on the actual flash, and measured worst-case latency
through append, full-store handling, page migration and recovery. No bounded
in-motion persistence timing or hardware qualification is claimed. Product code
owns fallback diagnostics, safe motion state, save authorization and retry policy.

Crates.io search found no `polis-settings` package; publication/name ownership
has not been exercised. No package was published and no changes were committed.
