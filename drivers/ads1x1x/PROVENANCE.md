# Source provenance

Reviewed and reworked on 2026-10-11 from
`2043-UTSuites/ControlCode/ads1015-rs` at repository revision
`b0173ce5e084da9e00ec338c1572995b73ecc8d5` (clean source checkout).

The source had no license declaration at import. On 2026-10-11, the owner
selected MIT for this shared package; see LICENSE-MIT. The original source
checkout and its recorded hashes are unchanged.

The original register constants, ADS1015 encodings, signed conversion behavior
and blocking/async I²C approach informed this implementation. It is substantially
rewritten, with model-specific rates and capabilities, complete configuration,
no cached register state, bounded polling, and consumer-owned GPIO waiting.
Definitions were checked against TI ADS101x SBAS473F (January 2025) and ADS111x
SBAS444E (December 2024). See README.md for links and API migration guidance.

The shared driver now exposes only the async API; the blocking implementation
and direct `embedded-hal` dependency were removed. Original source hashes below
record the imported reference, including its blocking implementation.

No product ADC task, board pins, Embassy patches, source lockfile or firmware
example was imported. Original product files remain unchanged.

## Original reference file hashes

| File | SHA-256 |
| --- | --- |
| `Cargo.toml` | `8d841b5b2f1e310d558879bb021a6a63d3f2da2d5aaf9b3133b2ede40864c90e` |
| `src/asynch.rs` | `34f00f7c010d49320d61f5fcb1825c21e3cfada24ac0a0eb815c878ad879d371` |
| `src/common.rs` | `f84881feb78d0c1dc0643ad888d0f5a528f869b919a5edff6dc3182da31309bf` |
| `src/lib.rs` | `753339212795adf91f0e392cf2c5139fb33ab1aab317737752af8e62c96c8581` |
| `src/sync.rs` | `260b818676824c6f970bb52af204e5ae1344842de1447b925cc7fafbbe04b96a` |

## Usage example adaptation

`examples/embassy.rs` was added on 2026-10-11 using
`2043-UTSuites/ControlCode/ads1015-rs/example/embassy-rp/src/bin/asynch.rs`
from the source checkout (recorded repository revision `b0173ce5e084da9e00ec338c1572995b73ecc8d5`).
It reduces the source setup to a single usage path with this package's current
API; product configuration, diagnostics, and test harnesses are not imported.
Source checkouts are unchanged. Package-local linker support follows the
existing `pio-ws2813` example. Original reference hashes above are unchanged.
