# Changelog

## 0.1.0 — Unreleased

- Add an allocation-free 128×32 SSD1306 adapter using upstream 0.10's async API.
- Provide a 512-byte framebuffer, embedded-graphics drawing, both I²C addresses,
  local clear/inspection, and bus release.
- Reset addressing and transmit full frames on every flush for repeatable retry
  after failed or cancelled transfers; retain pixels during reinitialization.
- Add an optional allocation-free Pico/Pico 2 example with 28-pixel text scrolling
  above a 2-pixel gap and animated 2-pixel bars.
- Add host regression tests, embedded build integration and package provenance.
