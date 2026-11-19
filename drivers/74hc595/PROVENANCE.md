# Source provenance

Added on 2026-11-18 as an original MIT implementation for 74HC595/74HCT595.
The device specification is [Nexperia 74HC595; 74HCT595, Rev. 12, 20 March 2024](https://assets.nexperia.com/documents/data-sheet/74HC_HCT595.pdf),
sections 6–11: pinning, function table, operating conditions, and timing.

Reference API comparisons:

- [Adafruit CircuitPython 74HC595](https://github.com/adafruit/Adafruit_CircuitPython_74HC595/tree/8fe7ffa3cd0a7b656575c667b69155497e9137cc),
  revision `8fe7ffa3cd0a7b656575c667b69155497e9137cc`. Reviewed `adafruit_74hc595.py`,
  whose header declares MIT and credits Kattni Rembor / Adafruit Industries.
  Its cached output and per-pin interface informed the API comparison.
- [tactile-eng/driver-74hc595](https://github.com/tactile-eng/driver-74hc595/tree/1db82ad4ebfdc46230efa86a0d1a698931e8e1a9),
  revision `1db82ad4ebfdc46230efa86a0d1a698931e8e1a9`, identified as Apache-2.0
  by its repository. Reviewed `src/lib.rs`: separate load/latch/reset operations
  and generic async HAL resources.

Neither reference's source was copied, translated or vendored. This package
implements a complete fixed-length frame operation with explicit SPI flush,
latch timing, no output cache, and caller-owned OE/MR. External libraries retain
their own licenses and are not dependencies.

The manifest pattern, build script, linker layouts and license come from this
repository's MIT `drivers/xca953xa` package at revision
`11a109856660007f5241e716a0921cbb56735d16`. The Embassy SPI/executor setup follows
`drivers/tmc5160/examples/embassy.rs` at the same revision. Copied support files:

| File | SHA-256 |
| --- | --- |
| `build.rs` | `64f92c53a6cdef4478eef260513d8fc1a403202e55298647844396a1e9cb91b7` |
| `examples/memory-rp2040.x` | `9d6c3043cbc579068ad7de0a6d2e755271bd3a0782985ecbcc66753119cba3de` |
| `examples/memory-rp2350.x` | `fe763dce8a1f9f4c309da276397526bcc68b008daf3e30b213abddd8b014ca7f` |
| `LICENSE-MIT` | `faa944eda08fb4a7afa849f83166fe4993812d8058a86a8c423c325f8c569dbe` |

The crates.io sparse index entry `po/li/polis-74hc595` returned HTTP 404 on
2026-11-18. This does not reserve the name; recheck before publication. No package
has been published and no downstream product has been migrated.

## Controller counter example

On 2026-11-18 the example was adapted at the user's request to
`2052-HipCircumduction/hc-boards/Controller`. The local `gpio.json`,
`schematic.py`, and `Docs/ControllerDesign.md` establish the Pico pins, common
cathode polarity, resistor-network output swaps, U4 → U3 chain order, and
always-enabled OE wiring. These board files are untracked, so their content
hashes identify the reviewed snapshot:

| Source | SHA-256 |
| --- | --- |
| `Controller/gpio.json` | `848856945a4c0b581ee86cc83ace78102d78dc31caa24f52624ef6c29479004b` |
| `Controller/schematic.py` | `a0662aec382ef12334ba9c2df3e7d2c053d3a6ba9eb4797d4ebc9168a60e2d23` |
| `Docs/ControllerDesign.md` | `4545390f8e43bef13e614aae714f73ebe3f3f197f888523db8f412d0a6c92a7f` |

PIO SPI setup follows this repository's MIT
`drivers/tmc5160/examples/embassy_pio.rs`. Board-specific segment mapping remains
in the focused example; the reusable driver and board sources are unchanged.
