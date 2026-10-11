// Adapted from embassy-rp 0.10.0, src/pio_programs/ws2812.rs.
// Copyright (c) Embassy project contributors
// Portions copyright (c) rp-rs organization
// SPDX-License-Identifier: MIT

use crate::RGBW;

const BIT_HZ: u64 = 800_000;
const START: u8 = 5;
const DATA: u8 = 7;
const LOW: u8 = 8;
const CYCLES_PER_BIT: u64 = (START + DATA + LOW) as u64;
const RESET_US: u64 = 250;

pub(crate) fn program() -> pio::Program<32> {
    let mut a = pio::Assembler::<32>::new_with_side_set(pio::SideSet::new(false, 1, false));
    let mut start = a.label();
    let mut zero = a.label();
    let mut end = a.label();
    a.bind(&mut start);
    // Autopull stalls here with the output low when there is no more data.
    a.out_with_delay_and_side_set(pio::OutDestination::X, 1, LOW - 1, 0);
    a.jmp_with_delay_and_side_set(pio::JmpCondition::XIsZero, &mut zero, START - 1, 1);
    a.jmp_with_delay_and_side_set(pio::JmpCondition::Always, &mut start, DATA - 1, 1);
    a.bind(&mut zero);
    a.nop_with_delay_and_side_set(DATA - 1, 0);
    a.bind(&mut end);
    a.assemble_with_wrap(end, start)
}

pub(crate) fn pack(color: RGBW<u8>) -> u32 {
    u32::from_be_bytes([color.g, color.r, color.b, color.a.0])
}

pub(crate) struct Timing {
    pub divider_256: u32,
    pub drain_and_reset_us: u64,
}

pub(crate) fn timing(sys_hz: u32) -> Timing {
    // Below this, divider quantization can violate the minimum symbol period.
    assert!(sys_hz >= 32_000_000, "SK6812 requires clk_sys >= 32 MHz");
    let sys_hz = u64::from(sys_hz);
    // Round up so the average bit rate never exceeds 800 kbit/s.
    let divider_256 = (sys_hz * 256).div_ceil(BIT_HZ * CYCLES_PER_BIT);

    // Joined TX FIFO: eight words, plus OSR, plus the bit held in X while
    // autopull can refill OSR. This covers both completed and aborted DMA.
    let tail_cycles = (9 * 32 + 1) * CYCLES_PER_BIT;
    // One extra system tick covers the fractional divider's starting phase.
    let tail_sys_ticks = (tail_cycles * divider_256).div_ceil(256) + 1;
    let drain_us = (tail_sys_ticks * 1_000_000).div_ceil(sys_hz);
    Timing {
        divider_256: divider_256 as u32,
        drain_and_reset_us: drain_us + RESET_US,
    }
}

#[cfg(test)]
mod tests;
