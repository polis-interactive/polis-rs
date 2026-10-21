# Changelog

## 0.1.0 — Unreleased

- Extract the async SH1106 I²C driver from FishLamp into `polis-sh1106`.
- Correct visible geometry to 128×64 and transmit exactly 128 columns per page.
- Use the SH1106 DC-DC command and upload pixels before display enable.
- Support both addresses, bus recovery access, framebuffer inspection and fast clear.
- Add regression tests, provenance and an opt-in Mousefood/Ratatui-core example
  with Pico/Pico 2 linker support. Normal builds remain allocation-free `no_std`.

Migration: replace the local `sh1106::Sh1106Async` import with
`polis_sh1106::Sh1106Async`. `new`, `init`, `flush` and `clear_buffer` remain.
Drawing now reports 128×64; x coordinates 128–131 are clipped. `init()` performs
a full framebuffer upload before enabling the display, adding bus traffic to
initialization. Existing local pixels are retained across reinitialization.
