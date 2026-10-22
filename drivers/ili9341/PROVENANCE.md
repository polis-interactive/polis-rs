# Source provenance

The RGB332 framebuffer and PIO adapter are original MIT code. The design request
referenced [moefh/pico-ili9341](https://github.com/moefh/pico-ili9341) as precedent
for saving framebuffer RAM with RGB332. Its README was consulted; no GPL-2.0
source or wiring implementation was copied. This implementation uses ordinary
D0..D7 wiring and bit-replicated PIO expansion, not the reference's zero-filled
color components and remapped parallel pins.

Added on 2026-10-20 as an original polis-rs integration around the published
`lcd-async` 0.1.3 dependency. No local product driver was extracted and no
upstream implementation source was copied or modified. The package follows
the repository's buffered-display API pattern; example build/linker scaffolding
comes from this repository's MIT `drivers/sh1106` package.

## Upstream implementation

- Registry package: <https://crates.io/crates/lcd-async/0.1.3>
- Repository: <https://github.com/okhsunrog/lcd-async>
- Registry `.cargo_vcs_info.json` revision: `ab6b71d677cae88a3f49d29988c4cc91d3db3ff8`.
- License: MIT. Upstream identifies itself as an async fork of mipidsi.

Initialization, reset timing, SPI/DC handling, window commands and raw-framebuffer
drawing remain upstream. Inspected dependency files (not vendored here):

| File | SHA-256 |
| --- | --- |
| `src/lib.rs` | `02e94090d8a9c0a893dc1c2493416a6404c2eed58b95236fa9b9aba6a1fc1373` |
| `src/models/ili9341.rs` | `4e5ac7507a510c68b294e43a789cf68a3d7c862f1283000f6c8032ac40ea85ee` |
| `src/models/ili934x.rs` | `f4685746b11f357f384272abfba4a85c83f5b85eee64b312af9d73bf352362d1` |
| `src/interface/spi.rs` | `a1513ffd0ac0a8ecd31a222193a69e3f6bb384e4339ef212711b8fff27704822` |
| `src/raw_framebuf.rs` | `ec1ae6a53147e4f81a578d9caa022d3ee8efbd8f64d6af61ff63820459292617` |

The local implementation fixes the supported geometry at 320×240 landscape
RGB565, borrows caller-owned framebuffer memory, and groups full uploads into
eight-row strips. Module-dependent landscape direction, color order and inversion
are passed through to upstream initialization. Tests and the display-only
Mousefood/Ratatui-core example are original additions. Touch and SD support are
explicitly excluded. Source product repositories are unchanged.

The optional `parallel` module is an original adapter to `polis-pio-i8080`.
Its PIO waveform, DMA/completion handling, and cancellation cleanup are original
code in that separate MIT package. The parallel Mousefood example reuses this
package's SPI example UI and linker scaffolding. No upstream GPIO-parallel
implementation was copied.

## Hardware references

The requested module is the Hosyond 2.4-inch SPI ILI9341, Amazon ASIN B09XHRKFMM.
The Amazon page could not be fetched directly. Its identity was cross-checked
against indexed listing descriptions; the primary hardware reference is
[Hosyond's support site](https://hosyond.com/), which links the 2.4-inch SPI model
to [LCDWiki MSP2402](https://www.lcdwiki.com/2.4inch_SPI_Module_ILI9341_SKU:MSP2402).
That reference describes the SPI interface, power/logic levels, and separate touch
signals. No claim is made that every board revision is electrically identical.

## Licensing and distribution

The rocket example added on 2026-10-22 uses the user-supplied
`assets/rocket_sprite/output/rocket.rs` decoder and `rocket.bin` sprite, copied
verbatim into `examples/rocket_support/` so package builds are self-contained.
The source asset README describes a manually reconstructed 11×21 pixel grid from
a reference image. No separate asset license was supplied; this provenance note
does not assert ownership or grant additional rights to the artwork. Its source
and hashes are recorded in `examples/rocket_support/README.txt`. The new scene,
renderer, simulation tests, preview and firmware integration are original example
code under the repository's MIT policy. Original asset files remain unchanged.

Original adapter/example/test code uses the owner's established MIT policy and
includes LICENSE-MIT. `lcd-async` remains a registry dependency carrying its own
notices; this package does not relicense its implementation.

The crates.io sparse-index path `po/li/polis-ili9341` returned HTTP 404 on
2026-10-20. Recheck name availability and add the real repository URL before
publication. No package has been published or name reserved by this work.
