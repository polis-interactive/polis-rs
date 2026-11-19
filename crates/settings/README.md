# polis-settings

Allocation-free `no_std` typed, versioned settings over **sequential-storage
8.0.2**, pinned exactly. The caller supplies flash, an erase-aligned partition,
four-byte-aligned scratch of at least one erase page, and an encoding buffer.
Normal library builds select no MCU, runtime, allocator, task or global.

## Contract

Implement `Record` with a stable caller-assigned `u16` key, schema version, and
deterministic payload encoder/decoder. The decoder must consume the whole
payload and reject trailing bytes. Records are independently consistent; no
multi-key transactions are provided. Do not reuse a key for an unrelated record.

`Settings::new` performs no I/O. It validates range/capacity, erase alignment,
minimum two-page capacity, backend geometry and buffer requirements. Addresses
are offsets relative to the supplied flash object, including when that object
is itself a partition. Runtime validation cannot detect firmware overlap:
the consumer must reserve the same region in its linker layout.

| Operation | Outcome / responsibility |
| --- | --- |
| `load::<R>()` | `Load::Present(R)`, `Missing`, or `Unusable(RecordProblem)`; backend errors are returned separately |
| `save(&snapshot)` | Reads the existing record and refuses unsupported/malformed records; `Ok(())` acknowledges this snapshot only |
| `replace(&snapshot)` | Explicit product authorization to replace one key, including a different schema; use after product-owned migration |
| `erase_all()` | Explicit destructive partition reset; returns failures, never runs as a wrapper's load-error fallback |
| `free()` | Returns the flash and borrowed buffers for reconstruction with a fresh backend cache |

The uncached backend uses page-sized scratch so a newer, larger unknown record
can be inspected and moved during page migration even when the current encoding
buffer is small. The encoding buffer includes a three-byte envelope header.
Supported flash read/write sizes are powers of two up to 32 bytes. Flash-driver
requirements beyond the storage traits still apply.

## Record format and compatibility

The map key is a little-endian `u16`. The raw map value is:

| Offset | Content |
| --- | --- |
| 0 | Settings envelope format version: `1` |
| 1..3 | Application schema version, little-endian `u16` |
| 3.. | Application-defined payload, with no padding inside the value |

Backend flash-format compatibility, this envelope version and each application's
schema are separate contracts. No second journal or checksum is added. See
[BACKEND.md](BACKEND.md) for the compatibility review and recovery limitations.

For a known old schema, load an old `Record` type with the same key, validate and
migrate in the application, then explicitly `replace` using the new type. An
unknown schema or malformed payload remains preserved by ordinary `save`, even
when the product selects a validated RAM fallback. Defaults and RPM clamping
are application policy; storing a setting never authorizes motion.

## Scheduling, failures and cancellation

**Every operation, including load, may repair flash and program/erase pages.**
The wrapper does not erase on load failure, but backend recovery can erase pages.
There is no read-only recovery mode. Async does not imply a yield or bounded
latency: RP flash writes/erases are blocking. The caller must authorize flash
maintenance time and qualify worst-case migration/recovery/full-store timing.

Backend CRC failures can cause an item to be skipped, yielding an older intact
value or `Missing`. Neither proves the store was never initialized. The wrapper
reports errors the backend exposes; it cannot promise a diagnostic for every
physical corruption. Missing/unsupported/invalid **application records** and
reported storage failures remain distinct. Do not automatically reset the store.

After error or cancellation, the attempted save may or may not have committed.
Retain the unsaved snapshot/generation and reconcile or retry according to product
policy. Subsequent operations use the backend's recovery. Explicit reset is
different: interruption may leave only part of the partition erased; retry the
authorized reset before claiming an empty store.

The application owns coalescing, unchanged-value suppression and dirty state.
For concurrent input, capture `(snapshot, generation)` before saving; on success
clear dirty **only if the current generation still equals the saved generation**.
Otherwise the newer selection remains unsaved. Do not allow generations to wrap
while a save is outstanding. Failed saves never clear dirty. A one-second quiet
interval and retry deadlines belong in the product manager, not this library.

## Minimal codec

```rust
use polis_settings::{CodecError, Record};

struct Preference(u16);
impl Record for Preference {
    const KEY: u16 = 1; // Assigned by the application.
    const VERSION: u16 = 1;

    fn encode(&self, output: &mut [u8]) -> Result<usize, CodecError> {
        output.get_mut(..2).ok_or(CodecError::BufferTooSmall)?
            .copy_from_slice(&self.0.to_le_bytes());
        Ok(2)
    }

    fn decode(payload: &[u8]) -> Result<Self, CodecError> {
        let bytes = payload.try_into().map_err(|_| CodecError::InvalidEncoding)?;
        Ok(Self(u16::from_le_bytes(bytes)))
    }
}
```

## Validation and hardware example

```sh
cargo test --locked -p polis-settings
python3 tools/check.py build --platform all --package polis-settings
python3 tools/check.py clippy --platform all --package polis-settings
```

Host tests exercise the real pinned backend with a fault-injectable NOR model:
interrupted append/migration, torn programming/erase, pending-operation
cancellation, corruption, exhaustion, storage failures, unknown versions,
explicit migration/reset, buffer/partition validation, and guard regions.
See [VALIDATION.md](VALIDATION.md) for results and remaining target qualification.

`examples/embassy.rs` is a Pico/Pico 2 boot counter. **Running it writes the final
16 KiB of flash** (four sectors). Example linker scripts reserve that region and
export its bounds; runtime code derives offsets from those symbols and checks
the flash size. No automatic erase or factory reset occurs. Use a test board
with an erased example partition; unrelated pre-existing contents are not a
supported import format. Product flash sizes/layouts remain downstream.

```sh
cargo build --locked -p polis-settings --release --target thumbv6m-none-eabi --example embassy --features example-rp2040
probe-rs run --chip RP2040 .target/thumbv6m-none-eabi/release/examples/embassy

cargo build --locked -p polis-settings --release --target thumbv8m.main-none-eabihf --example embassy --features example-rp235xa
probe-rs run --chip RP2350 .target/thumbv8m.main-none-eabihf/release/examples/embassy
```

The example logs the persisted boot count or a diagnostic, then waits. Linked
images validate integration, not flash latency or power-fail behavior on hardware.
