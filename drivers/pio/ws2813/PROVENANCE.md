# Source provenance

Imported on 2026-10-11 from `1017-Slate/SlateCode/drivers/pio-ws2813`.
Source repository revision: `5cf31aea6bd683d2ac958ca31714f8b480218c4c`. The source package was clean at import.

Original license: `MIT`. Existing license files and source notices are retained.

Changes: package/import names, distribution metadata and README paths/commands.
Algorithms and wire behavior are preserved. Any subsequent implementation changes
are recorded in CHANGELOG.md. No product application or BSP is included.

## Original file hashes

| File | SHA-256 |
| --- | --- |
| `Cargo.toml` | `3a1e31df180745606de0c6b19abd08e94258198dc30e92dda7265ca1a1285a57` |
| `LICENSE-MIT` | `54640665da949939393c396c9bcde4433455cf3b27aa66caa5738e0ec6e2c556` |
| `README.md` | `04e27ced38399e129839c6c9dec469b7b00b1af22ea4f85ce6880f16d6ac6f03` |
| `build.rs` | `7a0211a0d615ecb77b78024d9e0f05f8d0f2d69028c124fff46c7652f9b11d57` |
| `examples/link_check.rs` | `e338e8d9016e9c8ad8aab40a1e1314c28f0965ae4a9104e67ab01ab4f8a1e7c9` |
| `examples/memory-rp2040.x` | `9d6c3043cbc579068ad7de0a6d2e755271bd3a0782985ecbcc66753119cba3de` |
| `examples/memory-rp2350.x` | `fe763dce8a1f9f4c309da276397526bcc68b008daf3e30b213abddd8b014ca7f` |
| `src/driver.rs` | `14fa95202de4cdc6bcb84afac6f297e01d71f3b06660cd5dd11cb58e67741628` |
| `src/lib.rs` | `bc34044e15ac7ddb5351dffc497b22e5f28836ce49522e6695f293746362b7ec` |
| `src/waveform/tests.rs` | `9687f3f0e65bcbbdacfb174777e5d07ec2bb2868a3a313f39f0408c552010404` |
| `src/waveform.rs` | `5817cff9f4f548a2c929ac39a46a9ae7481397fcded7befb635f90670370156d` |

## Usage example adaptation

`examples/embassy.rs` was added on 2026-10-11 using
`1017-Slate/SlateCode/bin/test-ws2813/src/{main,board}.rs`
from the source checkout (recorded repository revision `5cf31aea6bd683d2ac958ca31714f8b480218c4c`).
It reduces the source setup to a single usage path with this package's current
API; product configuration, diagnostics, and test harnesses are not imported.
Source checkouts are unchanged. Package-local linker support follows the
existing `pio-ws2813` example. Original reference hashes above are unchanged.
