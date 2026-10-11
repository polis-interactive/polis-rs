# Source provenance

Imported on 2026-10-11 from `1017-Slate/SlateCode/drivers/tmp1x2`.
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
| `Cargo.toml` | `064197ef0c7e0673d436e37b8f657fd8bf36117962d2a36d952f53737a4975ef` |
| `README.md` | `68bd882020df701cc6332670bd6ca348d9112a019f7ae781ed55d8932f7ef906` |
| `src/lib.rs` | `219a7818cc39e386a4ac67b27e78a53f8bfb10c932782cb152d856af24615375` |
| `src/tests.rs` | `f1eec7ef2a20e71b47422c1aecf952b94da6eabfa2624bffce964d231cbfd2c3` |

## Usage example adaptation

`examples/embassy.rs` was added on 2026-10-11 using
`1017-Slate/SlateCode/bin/test-tmp1x2/src/{main,board}.rs`
from the source checkout (recorded repository revision `5cf31aea6bd683d2ac958ca31714f8b480218c4c`).
It reduces the source setup to a single usage path with this package's current
API; product configuration, diagnostics, and test harnesses are not imported.
Source checkouts are unchanged. Package-local linker support follows the
existing `pio-ws2813` example. Original reference hashes above are unchanged.
