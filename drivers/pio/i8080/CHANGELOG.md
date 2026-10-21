# Changelog

## 0.1.0 — Unreleased

- Add a pinless 14-instruction RGB332 converter and two-SM pipeline with three
  independently FIFO-paced DMA channels and caller-owned alternating buffers.
- Preserve output timing and cancellation cleanup while overlapping conversion
  and transmission; test converter backpressure, byte order and cycle count.

- Add optional RGB332-to-RGB565 expansion using the same SM and DMA channel,
  24 extra instructions, bit-replicated color levels and direct byte-slice DMA.
- Test all 256 colors, timing, starvation, large counts and final WR completion.

- Add write-only 8-bit 8080 display signaling with RP2040/RP2350A PIO and DMA.
- Use integer timing dividers and a completion IRQ after the final WR hold time.
- Add cancellation cleanup, bounded DMA counts, and direct byte-slice transfers.
- Add waveform/timing tests and standalone Pico/Pico 2 logic-analyzer fixtures.
