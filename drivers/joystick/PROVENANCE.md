# Source provenance

Extracted on 2026-10-19 from the local checkout
`2050-hdmi-passthru/crates/hdmi-joystick`.
The source checkout HEAD was `2dddbceceda9921118e8e003baaab6860e7866fe`.
**The entire joystick package was untracked and is absent from that commit.**
This revision identifies the surrounding checkout only; it is not a committed
source revision for the extracted files. No source remote was configured.

The complete uncommitted addition consisted of `Cargo.toml`, `src/lib.rs`,
`src/ads.rs`, and `src/rp.rs`. Original API documentation and tests were inline;
there was no package README, changelog or license file. These exact pre-extraction
bytes were recorded before editing or removing anything:

| Source file | SHA-256 |
| --- | --- |
| `crates/hdmi-joystick/Cargo.toml` | `676b7b851891723a7222d7b706234cd602d7b4e50c8c95f743e749a53099900d` |
| `crates/hdmi-joystick/src/lib.rs` | `e9b3ad49317a80249623de62e38f6cf7563bfd73cf8bdc0b463b0861198c9430` |
| `crates/hdmi-joystick/src/ads.rs` | `750043b16e19efa77eebc724f316991ddafca52420b3047f4018ee3b6b07350e` |
| `crates/hdmi-joystick/src/rp.rs` | `8941bb2cfcaf5103674a430a1b39c8d8c335c7fd5444e7818a693f53a48a7f1c` |
| `firmware/control-surface/examples/onchip-joystick.rs` | `3e6b2e4bca545b2db2fdc8b73fa158a10b00bb80ed7d2869d2fabea7bba076c4` |

The on-chip example was also untracked at extraction. Its minimal acquisition
pattern was adapted to `examples/embassy.rs`, using the new package import and
explicitly illustrative calibration. Package-local build/linker scaffolding
comes from this repository's MIT `drivers/pio/uart` example. No upstream commit
is claimed for the untracked example or original joystick source.

## Licensing

The source package declared no license, and its source checkout had no license
file. Under the owner's polis-rs licensing policy for original code, the extracted
package declares MIT and includes LICENSE-MIT. This records the license selected
for this extraction; it does not claim the original untracked files had an MIT
declaration. No third-party source was copied into the implementation.
polis-ads1x1x and Embassy RP remain separate dependencies with their own notices.

## Changes at extraction

Production acquisition and arithmetic are preserved. The package/import name
changes to `polis-joystick` / `polis_joystick`. Cargo metadata, optional dependency
sources, chip/example feature forwarding, an incompatible-chip guard, and
missing-docs lint are added. The stale "kept local" module comment is replaced.
Original tests are retained; the shared-ADC test now instantiates two Joystick
objects explicitly, with extra rounding and first-axis-failure checks.

Board pins, initialization, supply assumptions, product calibration, buttons,
debounce, control-link types, profiles, deadlines and recovery stay in HDMI.
Its consumer changes are dependency/import migration and documentation; the
old package is removed only after both migrated RP2040 consumers link.

The local ADS driver implementation/manifest matches the consumer's previously
pinned polis-rs revision `c3abc0e5138b9af3081bd6ac0a2e04b34abb0cb4`;
only its README build-output path differs. A temporary local Cargo patch unifies
that Git dependency and this package's path dependency during integration.

Embassy RP 0.10.0 is tested; its registry source identifies revision
`84444a19eb57d978e3c09fcdc8e60cdd7278eb03`. No Embassy implementation was copied.
crates.io metadata returned HTTP 404 for both `polis-joystick` and
`polis-ads1x1x` on 2026-10-19. Neither package is assumed published; availability
must be checked again at release. No commit, push or publication was performed.
