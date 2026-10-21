# Source provenance

Added on 2026-10-20 as a 128×32 async I²C adapter around the published
[`ssd1306` 0.10.0](https://github.com/rust-embedded-community/ssd1306) dependency.
Upstream already supplies async support. No upstream source was copied or
vendored, and no external product checkout was modified.

The published archive's `.cargo_vcs_info.json` records revision
`27d63b412c850b2ac3f3c758a04347aa35d0b3c7`. The downloaded
`ssd1306-0.10.0.crate` SHA-256 is
`3ea6aac2d078bbc71d9b8ac3f657335311f3b6625e9a1a96ccc29f5abfa77c56`.
The dependency declares MIT OR Apache-2.0 and retains its own notices.

## Local implementation

The adapter's framebuffer, graphics traits, typed address and test patterns are
adapted from this workspace's MIT `drivers/sh1106`. The example and package-local
build/linker scaffolding use that package's Pico/Pico 2 setup. These source files
were uncommitted at adaptation; no committed source revision is claimed:

| Local source | SHA-256 before adaptation |
| --- | --- |
| `drivers/sh1106/src/lib.rs` | `0a845f0d270ae33c7894b15c258ae0c6995313f26ab39736c02fd8de459b03f8` |
| `drivers/sh1106/src/tests.rs` | `01377c80517f48101c3da8e20d32c1c3cefc2bebc806535964d63d236333e2b6` |
| `drivers/sh1106/examples/embassy.rs` | `4bc0c0d6ef3d91a639c4ea5568dc45b9b44ed285de19884827b73d268a449fc8` |

This package owns a 512-byte buffer and a concrete upstream async controller
in basic mode. Upstream handles device initialization, horizontal window commands
and chunked I²C transfer. The adapter resets the whole window on every flush and
retains pixels through failure/cancellation. It avoids upstream buffered mode's
pre-transfer dirty-bound reset in 0.10.0. UI dependencies remain example-only.
The new adapter code follows the owner's MIT policy and includes LICENSE-MIT.

The crates.io sparse-index entry `po/li/polis-ssd1306` returned HTTP 404 on
2026-10-20. This does not reserve the name. Recheck availability and supply the
repository URL before release under the workspace publishing procedure.
