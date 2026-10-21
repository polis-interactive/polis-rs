# Adding and adopting shared libraries

Package READMEs describe supported APIs and current limitations;
`PROVENANCE.md` files record origins. See [development](development.md) for
reproducible checks and [publishing](publishing.md) for archive verification.

## For each new library

1. Identify the reusable boundary and supported devices. Keep product protocols,
   firmware, BSPs and application policy in their consumer repositories.
2. Record the source revision, local differences, file hashes and license terms.
   Add the implementation, tests, package README and changelog together.
3. Validate host behavior, supported target builds, relevant linked consumers and
   the standalone package archive. Document outstanding hardware checks in the
   package README; report check results in the PR or commit description.

## Consumer adoption

All twenty-three workspace libraries now use `polis-<purpose>` Cargo package names.
Update dependency keys and Rust imports together: `framing` becomes
`polis-framing` in Cargo.toml and `polis_framing` in Rust; the same prefix applies
to every other library. Package directories, public items, transport feature
names, and wire formats are unchanged. Update Cargo `-p` / `--package` selections
and dependency feature forwarding to use the new package names too.

For a staged consumer migration, an explicit dependency alias can preserve old
imports while selecting the renamed package:

```toml
[dependencies]
framing = { package = "polis-framing", git = "<polis-rs repository URL>", rev = "<full commit SHA>" }
```

Switch consumers separately to a pinned upstream revision or published version,
validate their images, then retire local copies. Discovery consumers adopt the
shared schema with their own application ID. Package READMEs document API
migration requirements.

Release prerequisites and dependency ordering live in [publishing](publishing.md);
package boundaries and dependency examples live in [architecture](architecture.md).

## HDMI joystick extraction

`drivers/joystick` preserves the former `hdmi-joystick` API under `polis-joystick`.
The source files were untracked in HDMI, so package provenance records their
hashes separately from the surrounding checkout revision. Until this extraction
has a real committed revision or release, HDMI validates it through documented
local Cargo patches, including one ADS driver identity shared by BSP and adapter.
Do not substitute an older polis-rs commit that lacks the new package.

## SH1106 extraction

`drivers/sh1106` extracts FishLamp's async 128×64 OLED driver as `polis-sh1106`.
The API retains `Sh1106Async::new`, `init`, `flush` and `clear_buffer`; migrate
imports to `polis_sh1106::Sh1106Async`. The visible width is corrected from 132
to 128, the SH1106 DC-DC command replaces an SSD1306 command, and initialization
uploads the framebuffer before display enable. Mousefood remains compatible
through embedded-graphics traits and is enabled only for the example, alongside
Ratatui core. The source product has not been migrated. Package provenance,
regression tests and Pico/Pico 2 linked-example commands accompany the driver.

## SSD1306 integration

`drivers/ssd1306` provides `polis-ssd1306` for 128×32 OLED modules. It uses
`ssd1306` 0.10's existing async controller with a local 512-byte framebuffer;
no upstream fork is required. `Ssd1306Async::{new, init, flush, clear_buffer}`
mirrors the SH1106 usage pattern. Flush resets horizontal addressing and the full
window before sending pixels, so retries retain the complete frame. Unlike
SH1106, errors use upstream `DisplayError` and the column offset is zero.
The package includes a Mousefood/Ratatui-core example and both Pico linker layouts.
No downstream product has been migrated or physical hardware qualified.

## ILI9341 integration

`drivers/ili9341` provides `polis-ili9341` for 320×240 landscape RGB565 displays.
It reuses `lcd-async` 0.1.3 for async ILI9341 initialization and SPI transfers.
Optional RP2040/RP2350A features adapt the reusable `polis-pio-i8080` write-only
parallel transport; this bus owns no display commands or framebuffer.
Unlike the small OLED drivers, it borrows a caller-owned 150 KiB framebuffer;
`Ili9341Async::new(...).await` resets, configures and uploads the current pixels.
Drawing remains synchronous and `flush().await` sends the frame in eight-row
strips. The package includes a display-only Mousefood/Ratatui-core Pico/Pico 2
SPI and parallel examples. Touch input and SD-card support are excluded. No downstream product
has been migrated or physical hardware qualified.
