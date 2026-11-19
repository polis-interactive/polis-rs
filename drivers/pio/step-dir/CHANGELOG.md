# Changelog

## 0.1.0 — Unreleased

- Add no_std, allocation-free STEP/DIR execution for RP2040/RP2350A.
- DMA-fed finite segments, bounded commitment, exact tick configuration,
  seamless period/direction boundaries and low-only delay commands.
- End/starvation latches, partial emitted-edge accounting and cancellation
  cleanup requiring explicit reset before restarting.
- Host instruction-model tests and a disabled-motor Pico/Pico 2 fixture.
- Hardware qualification and exact Hall-edge capture remain outstanding.
