# Source provenance

Imported on 2026-10-11 from `1017-Slate/SlateCode/drivers/pio-uart`.
Source repository revision: `5cf31aea6bd683d2ac958ca31714f8b480218c4c`. The source package was clean at import.

The source had no license declaration at import. On 2026-10-11, the owner
selected MIT for this shared package; see LICENSE-MIT. The original source
checkout and its recorded hashes are unchanged.

Changes: package/import names, distribution metadata and README paths/commands.
Algorithms and wire behavior are preserved. Any subsequent implementation changes
are recorded in CHANGELOG.md. No product application or BSP is included.

## Original file hashes

| File | SHA-256 |
| --- | --- |
| `Cargo.toml` | `2f79cb7e60a53388110eac1054d318a15d564584f53a611560875ba4bc6d8f0f` |
| `README.md` | `7be3854883255b664a9b29c52482f849fc5a901b4b0b9c01238f197c13954b64` |
| `src/driver.rs` | `e3ce42fe5bf5eea33342880c5a1d2c15462ac2dc884b85e455efb18e0b25433d` |
| `src/lib.rs` | `22abdec9e478231c91b08e2d73d107ab909791cefb51eb27451e546eef6949cd` |
| `src/timing.rs` | `8293697e3fd5ec2ca9a8d48a82a37a7b68e811c3f8a7abdc31c0769732f1e008` |

## Usage example adaptation

`examples/embassy.rs` was added on 2026-10-11 using
`1017-Slate/SlateCode/bin/test-rs485/src/board.rs`
from the source checkout (recorded repository revision `5cf31aea6bd683d2ac958ca31714f8b480218c4c`).
It reduces the source setup to a single usage path with this package's current
API; product configuration, diagnostics, and test harnesses are not imported.
Source checkouts are unchanged. Package-local linker support follows the
existing `pio-ws2813` example. Original reference hashes above are unchanged.
