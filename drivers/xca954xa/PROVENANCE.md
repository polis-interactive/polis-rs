# Source provenance

Added on 2026-10-25 as an original MIT implementation of the PCA9548A control
register and async channel-bus adapter. Device behavior follows Texas Instruments
PCA9548A datasheet SCPS143G (March 2021), sections 8.6.1–8.6.3:
<https://www.ti.com/lit/ds/symlink/pca9548a.pdf>.

Renamed before release from `polis-pca9548a` to the family package
`polis-xca954xa`, with driver type `Xca9548a`. TI TCA9548A datasheet SCPS207H
(September 2024), register-map section, was reviewed to confirm its matching
address range, channel mask and STOP-based activation:
<https://www.ti.com/lit/ds/symlink/tca9548a.pdf>.
PCA9548A and TCA9548A are supported at the protocol level; no claim of identical
electrical limits or hardware qualification is made.

The public API design was compared with `eldruin/xca9548a-rs` (published crate
1.0.0) and Adafruit's CircuitPython TCA9548A library. No source from either project
was copied, translated or vendored. This implementation adds async trait-based
channels, optional Embassy synchronization, and conservative cache invalidation
on errors/cancellation. It does not claim family-wide or cascaded-mux support.

The examples consume this workspace's `polis-ssd1306` driver as a normal dependency.
The build script and memory layouts were copied from the MIT `drivers/ssd1306`
package at repository revision `01a25969b8c4ab6453316b73d533e478760ddd88`:

| Source | SHA-256 before copying |
| --- | --- |
| `drivers/ssd1306/build.rs` | `64f92c53a6cdef4478eef260513d8fc1a403202e55298647844396a1e9cb91b7` |
| `drivers/ssd1306/examples/memory-rp2040.x` | `9d6c3043cbc579068ad7de0a6d2e755271bd3a0782985ecbcc66753119cba3de` |
| `drivers/ssd1306/examples/memory-rp2350.x` | `fe763dce8a1f9f4c309da276397526bcc68b008daf3e30b213abddd8b014ca7f` |

The crate includes the repository's MIT notice. Dependencies retain their own
licenses and are not vendored.

The crates.io sparse-index entry `po/li/polis-xca954xa` returned HTTP 404 on
2026-10-25. This does not reserve the name. Recheck before release and supply
the real repository URL under the workspace publishing procedure. No package
has been published and no physical hardware qualification is claimed.
