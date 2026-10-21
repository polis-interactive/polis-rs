# Source provenance

Extracted on 2026-10-20 from
`2045-FishLamp-Engineering/CuttingCode/cc-embedded/src/sh1106.rs`.
Source repository revision: `593be535146d9808ca8dd6cdef5eaa433289995e`.
The driver was tracked and unchanged relative to that revision at extraction.
The source checkout is unchanged by this port.

| Source file | SHA-256 |
| --- | --- |
| `CuttingCode/cc-embedded/src/sh1106.rs` | `219a386776c8ba15385c5be57b6d45308344b85f981d3c22f175c90e83a9e0be` |
| `CuttingCode/cc-embedded/src/screen.rs` | `b4038da2df06f91c05228ca8d869797bb4b08583d55ef8fb4842d19ad0791247` |

`screen.rs` supplied the Mousefood/Terminal/draw/async-flush integration pattern.
Product state, watchers, widgets, pins and task policy were not imported. The
minimal Pico/Pico 2 example is new; build/linker scaffolding is reused from this
repository's MIT `drivers/tmp1x2` example.

## Licensing

The source manifest declared no license, and no license file was found in its
repository. Under the owner's polis-rs licensing policy for original code, this
extracted package declares MIT and includes LICENSE-MIT. This records the license
for the shared package, not a pre-existing source license declaration. No code
from the separate crates.io `sh1106` driver was copied.

## Changes at extraction

The driver is now an independent `no_std` package with direct
embedded-graphics-core traits. The buffer/visible width is corrected from 132 to
128 columns (1,024 bytes), retaining the two-column controller RAM offset.
Initialization replaces the SSD1306 charge-pump command with SH1106 `AD 8B`
and uploads the framebuffer before enabling the panel. Other module settings
are retained. Both supported I²C addresses, framebuffer inspection, bus access,
bus release and efficient binary-color clearing are added. No dirty tracking or
automatic retries are introduced. Tests and documentation are new.

Mousefood/Ratatui and runtime/allocation dependencies are optional and selected
only by example features. The source's Mousefood 0.4 integration is retained;
the example uses ratatui-core 0.1.2 to avoid the RP2040-incompatible Arc use in
ratatui-widgets 0.3.2. See README.md for validation commands and hardware limits.

On 2026-10-20, the crates.io sparse-index entry `po/li/polis-sh1106` returned
HTTP 404. The crates.io metadata API returned HTTP 403. No name reservation or
publication is claimed; recheck availability and supply the repository URL
before release, following the workspace publishing guide.
