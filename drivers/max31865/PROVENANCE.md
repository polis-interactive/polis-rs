# Source provenance

Imported on 2026-10-14 from
https://github.com/broose-goose/adafruit-max31865-rs at revision
`028f8e6dfd41019f3248699adaa506d3fb019c37` (clean checkout).
The upstream manifest declares BSD-3-Clause; it contains no standalone license
file, README or tests. This package retains that declared license and supplies
its standard text in LICENSE-BSD-3-Clause, with the source author's attribution.

The Rust source credits Adafruit_MAX31865. Adafruit's original repository was
inspected at revision `02dd3393d9750ca6e227be84b0e3b795a6941c36`:
https://github.com/adafruit/Adafruit_MAX31865 . Its source headers credit Limor
Fried/Ladyada for Adafruit Industries and require retaining their introductory
text. NOTICE includes the full source header and README verbatim. That checkout
has no standalone license file either; no upstream notice was removed or
replaced by this workspace's MIT default.

## Adaptation

The single portable driver retains the source's register protocol, calibration
presets, fault flags/messages and temperature algorithm. Blocking duplication
and the optional async gate are removed, fixing the source's unconditional use
of its optional async dependency. The new API validates calibration, writes
thresholds as 15-bit counts, preserves sample faults and exposes explicit
fault-cycle completion. Conversion waits cover the 50 Hz maximum (66 ms),
bias settling is configurable, and errors attempt bias shutdown. Cancellation
and partial writes require explicit caller recovery. See CHANGELOG.md and the
README migration notes for intentional behavior/API changes.

The minimal Embassy example adapts upstream SPI mode and measurement usage to
the workspace's current Embassy versions, DMA interrupts and runtime features.
It uses the collection's GP16–19 SPI wiring and package-local linker support
from `drivers/tmp1x2`, without upstream board power switching or product policy.
Source checkouts are unchanged. New host tests use yielding SPI/delay mocks and
independent forward Callendar–Van Dusen fixtures. Registers and timing were
checked against MAX31865 datasheet 19-6478 revision 3, July 2015.

## Original file hashes (SHA-256)

| Source | File | SHA-256 |
| --- | --- | --- |
| Rust | `Cargo.toml` | `a67401dc750c36603f4cd72c78cc82b087e074797bbe9fb3217fc4e47f36af8e` |
| Rust | `src/lib.rs` | `2482608938e71a38bfe5bb98c1f2156aad073d6a9dd6c36fb5c49f30878297e0` |
| Rust | `example/embassy-rp/src/main.rs` | `3f0efb460b6b6122a3ed1e84cd320b00208d38a1393c57e0b62f464883b46e47` |
| Adafruit | `Adafruit_MAX31865.cpp` | `c4a60c1ece61155ad342561d04ab786362dce34f9a8aa27cf0f86adf2677e41d` |
| Adafruit | `Adafruit_MAX31865.h` | `bdefc89c7914f0257741448a083551e7e0c72bbb54d347ca706531424b51abab` |
| Adafruit | `README.md` | `05f8db28943085424fc715836da3655029b01e8e506814a7c36b6864de2119cf` |

## Registry metadata

The crates.io API name check for `polis-max31865` returned HTTP 403 on
2026-10-14, so name availability remains unverified and must be rechecked before
publication. No name is reserved and no package has been published. The
collection has no established Git remote; no repository URL was invented.
