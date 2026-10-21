# Provenance

Original Rust and PIO implementation for polis-rs, licensed under MIT.
No controller driver or third-party PIO program was copied into this package.

RGB332 expansion is also original MIT PIO code. The README of
[moefh/pico-ili9341](https://github.com/moefh/pico-ili9341) supplied design
precedent for reduced framebuffer depth; none of its GPL-2.0 implementation was
copied. PIO instruction behavior and narrow-write replication follow the
[RP2040 datasheet](https://datasheets.raspberrypi.com/rp2040/rp2040-datasheet.pdf),
sections 2.1.4 and 3. The new path replicates color bits to fill RGB565 and uses
the existing consecutive eight-bit bus wiring.

Interface timing was checked against ILI Technology's ILI9341 datasheet V1.05,
section 18.3. The driver uses Embassy RP 0.10 public PIO/DMA APIs and the pio 0.3
assembler. Dependencies retain their own licenses.

This package supplies bus signaling only. Module selection, power circuitry,
pin wiring, and controller initialization remain outside the transport.

The two-SM converter and buffered DMA coordinator are original MIT code.
Raspberry Pi's [scanvideo design notes](https://github.com/raspberrypi/pico-extras/blob/master/src/common/pico_scanvideo/README.adoc)
were consulted for multi-SM IRQ coordination and buffer ownership, and its
[UART DMA example](https://github.com/raspberrypi/pico-examples/blob/master/pio/uart_dma/uart_dma.c)
for independently paced FIFO transfers. No source was copied. This implementation
uses separate FIFO DREQs and RAM handoff rather than scanvideo's scanline IRQs.

Registry-name lookup was attempted on 2026-10-20, but the crates.io API returned
HTTP 403 and the sparse-index lookup was inaccessible. Name availability remains
unverified; recheck it and add the real repository URL before publication.
Local offline package verification does not reserve a registry name.
