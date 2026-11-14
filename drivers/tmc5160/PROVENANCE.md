# Source provenance

Added on 2026-11-13 as original MIT Rust code. The driver is specific to
TMC5160/TMC5160A external STEP/DIR configuration through SPI. No external driver
implementation was copied, translated or vendored; register addresses, bit
definitions and current equations were checked against the primary datasheet.

## Primary specifications

- [TMC5160/A datasheet revision 1.18, 2023-MAR-01](https://www.analog.com/media/en/technical-documentation/data-sheets/TMC5160A_datasheet_rev1.18.pdf):
  sections 4 (SPI), 6 (registers), 7 (StealthChop), 9 (current/sense resistance),
  15 (STEP/DIR), and electrical characteristics (typical VSRT = 325 mV).
- [BTT TMC5160T Plus V1.0 manual and schematic](https://github.com/bigtreetech/BIGTREETECH-Stepper-Motor-Driver/tree/9f9680040e7f696f132b1c0d75a6f79c9f288b13/TMC5160T%20Plus),
  revision `9f9680040e7f696f132b1c0d75a6f79c9f288b13`. The manual specifies
  22 mΩ sense resistors and 10.6 A RMS maximum, with active cooling above 3 A;
  the V1.0 schematic identifies R5/R15 as 22 mΩ. This establishes board metadata,
  not a qualified motor profile or gate-drive/protection tune.

Downloaded BTT source hashes (files remain outside the package):

| Source | SHA-256 |
| --- | --- |
| `BIGTREETECH TMC5160T Plus User Manual.pdf` | `cd430eb324299461c93d73c4ec240d0199975b2808a5a8989580d511c3f980ab` |
| `Hardware/BIGTREETECH TMC5160T Plus V1.0-SCH.pdf` | `9d7302a6b868f878ab32dc36335ebca7855895eeb2f7df5e5afc50b2038a4b2c` |

## Reference implementations reviewed

- [ADI TMC-API](https://github.com/analogdevicesinc/TMC-API/tree/1de4a3aa7fe4bc5cbaa787d139d819b92c4ce410/tmc/ic/TMC5160),
  MIT, revision `1de4a3aa7fe4bc5cbaa787d139d819b92c4ce410`:
  register definitions/access permissions and the distinction between hardware
  reads and software shadows of write-only registers. Its sample presets are
  not adopted as board or motor defaults.
- [tommag/TMC5160_Arduino](https://github.com/tommag/TMC5160_Arduino/tree/0471224cc7e45d0ec6d3a89bacf4c7f79d845a5b),
  MIT, revision `0471224cc7e45d0ec6d3a89bacf4c7f79d845a5b`: reference only;
  Arduino, motion-controller and UART interfaces are not included.
- `embedded-hal-bus` 0.3.0 and Embassy RP 0.10.0 source were inspected for async
  SpiDevice/SpiBus compatibility and cancellation behavior. They remain ordinary
  dependencies, not vendored source.

## Repository example support

Build script, linker memory maps and MIT notice copied from `drivers/xca953xa`
at polis-rs revision `be443a888e32b07854094a46a87da1582d8d99a9`:

| Source | SHA-256 |
| --- | --- |
| `build.rs` | `64f92c53a6cdef4478eef260513d8fc1a403202e55298647844396a1e9cb91b7` |
| `examples/memory-rp2040.x` | `9d6c3043cbc579068ad7de0a6d2e755271bd3a0782985ecbcc66753119cba3de` |
| `examples/memory-rp2350.x` | `fe763dce8a1f9f4c309da276397526bcc68b008daf3e30b213abddd8b014ca7f` |

Example setup follows the existing `drivers/max31865/examples/embassy.rs`
pattern, with mode 3 and TMC diagnostics. No product/BSP source was extracted.

The crates.io sparse-index entry `po/li/polis-tmc5160` returned HTTP 404 on
2026-11-13. This does not reserve the name; recheck before publication. No
registry publication or physical hardware qualification is claimed.
