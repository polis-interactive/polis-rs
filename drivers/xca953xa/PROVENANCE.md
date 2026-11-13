# Source provenance

Added on 2026-11-13 as an original MIT implementation. Device behavior is derived
from TI's register and bus specifications, not from an imported driver:

| Device | Datasheet | Sections reviewed |
| --- | --- | --- |
| TCA9534 | [SCPS197D, October 2017](https://www.ti.com/lit/ds/symlink/tca9534.pdf) | 8.3–8.6: port/INT behavior, transfers, addressing and four registers |
| TCA9534A | [SCPS198C, February 2017](https://www.ti.com/lit/ds/symlink/tca9534a.pdf) | 8.3–8.6: matching map/behavior, alternate 0x38–0x3f addresses |
| PCA9534 | [SCPS124H, March 2021](https://www.ti.com/lit/ds/symlink/pca9534.pdf) | 9.2–9.3: port/INT behavior, addressing, map and transfers |
| PCA9534A | [SCPS141J, March 2021](https://www.ti.com/lit/ds/symlink/pca9534a.pdf) | 9.2–9.3: matching map/behavior, alternate addresses |

TI's canonical TCA9534 URL is used without the user's timestamp query.
Protocol support does not assert identical electrical limits or hardware qualification.

API/reference comparisons on 2026-11-13:

- [SparkFun I2C Expander Arduino Library](https://github.com/sparkfun/SparkFun_I2C_Expander_Arduino_Library/tree/49e762408e8fc0ca1d988f1596ecd295754bcd0e),
  revision `49e762408e8fc0ca1d988f1596ecd295754bcd0e`. Reviewed its multi-device scope,
  register access and per-pin API. Its source header carries a GPL notice.
- [Adancurusul/tca9534-driver-rs](https://github.com/Adancurusul/tca9534-driver-rs/tree/a100510cb0a7ed905829b28b1dd7729334842537),
  revision `a100510cb0a7ed905829b28b1dd7729334842537`. Reviewed public API and register
  declarations; not used as the source of truth or as a dependency.

No source from either external library was copied, translated or vendored.
This package deliberately uses direct async HAL traits, no device-state cache,
and an explicit initialization sequence. It does not inherit the references'
broader device support. External libraries retain their own licenses.

The package manifest pattern, build script and example linker layouts come from
the repository's MIT `drivers/modulino-buttons` package at repository revision
`ab1703d19c4a33b65ddb735092b6a651665dbe96`. The example uses the same Embassy RP
I²C/executor setup. Source hashes of copied support files:

| File | SHA-256 |
| --- | --- |
| `Cargo.toml` (adapted) | `570e338913d4d564e6316670896b5a730c5be2c151b4684d2bf39b7f8d62d1a5` |
| `examples/embassy.rs` (setup adapted) | `319ecf6e900ce4a9a10376b26f8da582599c4209b567f40efc246c55565839ba` |
| `build.rs` | `64f92c53a6cdef4478eef260513d8fc1a403202e55298647844396a1e9cb91b7` |
| `examples/memory-rp2040.x` | `9d6c3043cbc579068ad7de0a6d2e755271bd3a0782985ecbcc66753119cba3de` |
| `examples/memory-rp2350.x` | `fe763dce8a1f9f4c309da276397526bcc68b008daf3e30b213abddd8b014ca7f` |
| `LICENSE-MIT` | `faa944eda08fb4a7afa849f83166fe4993812d8058a86a8c423c325f8c569dbe` |

The crates.io sparse-index entry `po/li/polis-xca953xa` returned HTTP 404 on
2026-11-13. This does not reserve the name; recheck before publication.
No package has been published and no downstream product has been migrated.

## Controller panel example

On 2026-11-13 the example was adapted, at the user's request, to the local
`2052-HipCircumduction/hc-boards/Controller` pinout. `Controller/gpio.json` and
`Controller/schematic.py` supply the GPIO and U5 net assignments;
`Docs/ControllerDesign.md` documents the active-low buttons and the 5 V LED
low-sink/high-impedance policy. These source files are untracked in a repository
with no HEAD commit, so their inspected contents are identified by hashes:

| Reference | SHA-256 |
| --- | --- |
| `Controller/gpio.json` | `848856945a4c0b581ee86cc83ace78102d78dc31caa24f52624ef6c29479004b` |
| `Controller/schematic.py` | `a0662aec382ef12334ba9c2df3e7d2c053d3a6ba9eb4797d4ebc9168a60e2d23` |
| `Docs/ControllerDesign.md` | `4545390f8e43bef13e614aae714f73ebe3f3f197f888523db8f412d0a6c92a7f` |

Only wiring facts and required electrical behavior were used; no board generator,
hardware exports or product firmware were imported. The example is self-contained
and needs no downstream files to build. The library API and implementation are
unchanged by this adaptation.
