# Changelog

## 0.1.0 — Unreleased

- Precompute scaled color runs and exhaust coordinates for rocket rendering;
  verify equivalent pixels across rotations, scales, flame lengths and clipping.

- Raise the rocket example's PIO clock request to 50 MHz and show separate update,
  render and LCD elapsed times in its HUD/RTT to diagnose frame-rate bottlenecks.

- Add an allocation-free rocket graphics example with bounded random speed and
  headings, offscreen spawning/despawning, adjustable average population, an FPS
  overlay, and a native preview sharing the firmware simulation and renderer.

- Add configurable rotation after mounting/mirror correction, including 240×320
  portrait drawing, RGB565 strips and RGB332 row uploads. Rotate the RGB332 UI
  example 90° counterclockwise.

- Add optional horizontal mirroring for both framebuffer formats and landscape
  directions; enable it in the parallel examples for HiLetgo shield bring-up.

- Add a two-SM RGB332 pipeline adapter and full-frame/320×16 band benchmark for
  Pico/Pico 2, with alternating caller-owned buffers and cancellation/retry checks.
- Add clipped RGB332 `flush_rows`; RGB332 `flush` now uses a single full-frame
  window so conversion and output can overlap across transport DMA chunks.

- Add an opt-in 75 KiB RGB332 framebuffer with direct PIO + DMA expansion to
  RGB565 over 8080, compatible RGB565 drawing and native RGB332 buffer access.
- Add exhaustive conversion, framebuffer/recovery tests and a linked RGB332
  Mousefood example for Pico/Pico 2. SPI remains RGB565.

- Add a 320×240 RGB565 landscape adapter using upstream lcd-async ILI9341 support.
- Borrow a single 150 KiB framebuffer with embedded-graphics drawing and async flush.
- Bound pixel transfers to eight-row strips; preserve pixels through failure/retry.
- Support both landscape directions, RGB/BGR panel order and inversion settings.
- Add SPI/pin/delay regression tests and a display-only Mousefood/Ratatui-core
  Pico/Pico 2 example. No touchscreen or SD-card support is included.

- Add optional RP2040/RP2350A PIO + DMA 8-bit 8080 support through the reusable
  polis-pio-i8080 transport, with a parallel Mousefood/Ratatui example.
