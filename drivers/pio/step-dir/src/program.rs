// Original STEP/DIR program. See PROVENANCE.md for design references.
use crate::Config;

pub(crate) const STARVED: u8 = 22;
pub(crate) const FINISHED: u8 = 24;
pub(crate) const START: u8 = 26;

pub(crate) fn program(config: Config) -> pio::Program<32> {
    let mut program = pio::pio_asm!(
        ".side_set 1",
        ".wrap_target",
        "command:",
        "set x, 0 side 0",        // 0: fallback for nonblocking PULL
        "pull noblock side 0",    // 1: zero on empty, never repeats prior command
        "mov x, osr side 0",      // 2
        "jmp !x starved side 0",  // 3
        "out y, 16 side 0",       // 4: pulse count, not count-minus-one
        "out pins, 1 side 0",     // 5: DIR
        "out isr, 15 side 0",     // 6: low-delay loop count
        "mov x, isr side 0",      // 7
        "jmp !x finished side 0", // 8: explicit End sentinel
        "jmp !y wait_low side 0", // 9: bounded delay with no STEP edges
        "delay:",
        "jmp x-- delay side 0", // 10: d + 1 ticks
        "jmp emit side 0",      // 11
        "emit:",
        "jmp y-- high side 1", // 12: edge + accounting are indivisible
        "high:",
        "nop side 1",             // 13: patched high duration
        "nop side 0",             // 14: patched DIR hold duration
        "jmp !y complete side 0", // 15
        "mov x, isr side 0 [11]", // 16: match segment-boundary overhead
        "jmp delay side 0",       // 17
        "wait_low:",
        "jmp x-- wait_low side 0", // 18: d + 1 ticks, STEP remains low
        "complete:",
        "mov isr, y side 0",  // 19: zero, never a commanded-position claim
        "push block side 0",  // 20: four-entry admission guarantees RX space
        "jmp command side 0", // 21
        "starved:",
        "irq 0 rel side 0",   // 22
        "jmp starved side 0", // 23: even clearing IRQ cannot restart motion
        "finished:",
        "irq 0 rel side 0",    // 24
        "jmp finished side 0", // 25
        ".wrap",
        "nop side 0 [15]",    // 26: startup low guard, including after abort
        "jmp command side 0", // 27
    )
    .program;
    // One-bit side-set leaves four delay bits. Config validates bounds.
    program.code[13] |= u16::from(config.step_high_ticks - 2) << 8;
    program.code[14] |= u16::from(config.dir_hold_ticks - 1) << 8;
    program
}

#[cfg(test)]
mod tests;
