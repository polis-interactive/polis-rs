# Source provenance

Added 2026-10-19. Portable Rust decoding/policy, tests, and the Embassy adapter
are original polis-rs work under MIT; see LICENSE-MIT. No downstream firmware,
product protocol, or HDMI project files were imported or modified.
The linked example's build script and memory maps follow this repository's
MIT `drivers/pio/uart` scaffolding; its encoder composition is new.

## PIO algorithm

`src/program.rs` is adapted from
[Raspberry Pi pico-examples quadrature_encoder.pio](https://github.com/raspberrypi/pico-examples/blob/0d62f75bafc2c8120d3276c3343d1a9195e909e9/pio/quadrature_encoder/quadrature_encoder.pio)
at revision `0d62f75bafc2c8120d3276c3343d1a9195e909e9`.
Copyright (c) 2023 Raspberry Pi (Trading) Ltd.; SPDX BSD-3-Clause.
The upstream repository's LICENSE.TXT is retained in LICENSE-BSD-3-Clause,
including its repository-level 2020 notice. The source-level 2023 notice is
retained in program.rs. The combined package declares `MIT AND BSD-3-Clause`.

Local changes: express assembly through ordinary pio 0.3.0 Assembler methods;
append three initialization instructions to capture the actual phase and set
zero; start at address 24; join RX to eight words; explicitly configure B-low /
A-high pins, pull, synchronizers and caller-selected divider; provide async
bounded FIFO snapshot reads and separate portable net-travel scaling. The
upstream 24-instruction counting core and ten-cycle bound are preserved.

## Embassy inspection and dependency

Inspected the published `embassy-rp 0.10.0` registry source. Its
`.cargo_vcs_info.json` identifies revision
`84444a19eb57d978e3c09fcdc8e60cdd7278eb03` in
[embassy-rs/embassy](https://github.com/embassy-rs/embassy/tree/84444a19eb57d978e3c09fcdc8e60cdd7278eb03/embassy-rp).
Cargo.lock archive checksum:
`5d98f472894c1ecffac5596c657af5305c57282d29e8746d7fec033951931bc8`.

The rotary implementation waits high/low on one pin, samples two pins, and does
a blocking push. It reports single-edge directions, assumes adjacent A/B pin
ordering and a 12.5 kHz SM clock. It supplies neither cumulative retention during
FIFO saturation nor this library's four-state/startup behavior. No code from
that rotary implementation is incorporated. Embassy remains an external
MIT/Apache-2.0 dependency with its own licenses.

| Inspected original file | SHA-256 |
| --- | --- |
| pico-examples `pio/quadrature_encoder/quadrature_encoder.pio` | `9ac6b687dff969261ede9cb5e815971a9206a980a81990bbceec28e91401dd62` |
| pico-examples `LICENSE.TXT` | `483f865953435b66c443dee7558debe3cc3cf8fcbb6a112fd9fc6a795d53f1f6` |
| embassy-rp `src/pio_programs/rotary_encoder.rs` | `8a632870e49dd86e55bc4911778b5eb206e7e94b0a33fd3b0de575294b9df001` |
| embassy-rp `src/pio/mod.rs` | `a5eef22f6739d2ba86084716c42f8295b0c197212d8d92d6e00628151e7548b0` |

Instruction semantics were checked against the
[RP2040 datasheet](https://datasheets.raspberrypi.com/rp2040/rp2040-datasheet.pdf),
sections 3.4.2 and 3.4.4–3.4.7. Runtime execution on both chips still requires
hardware qualification; cross-compilation and the host interpreter do not
establish it.

The crates.io package-name API returned HTTP 404 for `polis-pio-encoder` on
2026-10-19. This is a point-in-time availability check, not a reservation.
Recheck ownership/availability before an explicitly requested publication.
