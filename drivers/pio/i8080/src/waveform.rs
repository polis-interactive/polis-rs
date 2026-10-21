pub(crate) fn program() -> pio::Program<32> {
    pio::pio_asm!(
        ".side_set 1",
        ".wrap_target",
        // Count is a full 32-bit FIFO word, followed by one FIFO word per byte.
        "pull block side 1",
        "out x, 32 side 1",
        "byte:",
        "pull block side 1",  // FIFO starvation can only extend WR high.
        "out pins, 8 side 1", // Data setup before falling WR.
        "nop side 0 [1]",
        "jmp x-- byte side 1 [1]", // Rising WR latches byte; hold for two cycles.
        "irq 0 rel side 1",        // Per-SM IRQ, after the final byte's hold time.
        ".wrap",
    )
    .program
}

// OUT shifts right; IN shifts left without autopush. Narrow DMA writes repeat
// RRR GGG BB four times in OSR. Read components from those repeated bytes and
// append RRR RR GGG GGG BB BB B to ISR. Its low 16 bits are the next RGB565
// pixel; old high bits are ignored. No lookup, scratch memory, or CPU expansion.
pub(crate) fn rgb332_program() -> pio::Program<32> {
    pio::pio_asm!(
        ".side_set 1",
        ".wrap_target",
        "pull block side 1",
        "out x, 32 side 1", // Pixel count minus one, not output byte count.
        "pixel:",
        "pull block side 1",
        "out null, 5 side 1",
        "in osr, 3 side 1", // RRR
        "out null, 1 side 1",
        "in osr, 2 side 1", // RR
        "out null, 4 side 1",
        "in osr, 3 side 1", // GGG
        "in osr, 3 side 1", // GGG
        "out null, 6 side 1",
        "in osr, 2 side 1", // BB
        "in osr, 2 side 1", // BB
        "out null, 1 side 1",
        "in osr, 1 side 1", // B
        "mov osr, isr side 1",
        "out null, 8 side 1",
        "out pins, 8 side 1", // High byte first.
        "nop side 0 [1]",
        "mov osr, isr side 1 [1]", // Restore low byte during high-byte hold.
        "out pins, 8 side 1",
        "nop side 0 [1]",
        "jmp x-- pixel side 1 [1]",
        "irq 0 rel side 1",
        ".wrap",
    )
    .program
}

// Pinless converter. Blocking FIFOs independently backpressure the input and
// output DMA channels. One halfword per pixel; RX DMA swaps its bytes for the
// raw-byte LCD output program. PUSH clears ISR between pixels.
pub(crate) fn rgb332_converter_program() -> pio::Program<32> {
    pio::pio_asm!(
        ".wrap_target",
        "pull block",
        "out null, 5",
        "in osr, 3",
        "out null, 1",
        "in osr, 2",
        "out null, 4",
        "in osr, 3",
        "in osr, 3",
        "out null, 6",
        "in osr, 2",
        "in osr, 2",
        "out null, 1",
        "in osr, 1",
        "push block",
        ".wrap",
    )
    .program
}

pub(crate) fn validate_pins(data: [u8; 8], wr: u8) -> Result<(), crate::ConfigError> {
    if data[0] > 24
        || data
            .iter()
            .enumerate()
            .any(|(i, &pin)| pin != data[0] + i as u8)
        || data.contains(&wr)
    {
        return Err(crate::ConfigError::Pins);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
