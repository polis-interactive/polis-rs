# Changelog

## 0.1.0 - Unreleased

- Add an allocation-free async 74HC595/74HCT595 driver with fixed-length chain
  writes, explicit SPI flush, timed latch pulses, and full-chain clearing.
- Preserve HAL errors and document cancellation/recovery and caller-owned OE/MR.
- Add behavioral host tests and a Pico/Pico 2 Embassy example with local linker
  inputs and opt-in runtime features.
- Adapt the example to the Controller's GP13/GP15/GP14 PIO SPI wiring and split
  segment mapping; count 00–99 once per second with decimal points off.
