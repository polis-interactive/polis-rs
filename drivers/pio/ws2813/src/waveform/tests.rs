extern crate std;

use pio::{
    Instruction, InstructionOperands, JmpCondition, MovDestination, MovOperation, MovSource,
    OutDestination,
};
use std::vec::Vec;

use super::*;

// Interpret the actual emitted instructions through one complete OSR word.
// This checks branch paths and delays, rather than just the timing constants.
fn emitted_bits(mut word: u32) -> Vec<(u32, u32, u32)> {
    let program = program();
    assert_eq!(program.code.len(), 4);
    let mut bits = Vec::new();
    for _ in 0..24 {
        let mut pc = program.wrap.target;
        let mut x = 0;
        let mut high_cycles = 0;
        let mut total_cycles = 0;
        loop {
            let instruction =
                Instruction::decode(program.code[pc as usize], program.side_set).unwrap();
            let cycles = u32::from(instruction.delay) + 1;
            total_cycles += cycles;
            if instruction.side_set == Some(1) {
                high_cycles += cycles;
            }
            let mut next = if pc == program.wrap.source {
                program.wrap.target
            } else {
                pc + 1
            };
            match instruction.operands {
                InstructionOperands::OUT {
                    destination: OutDestination::X,
                    bit_count: 1,
                } => {
                    assert_eq!(instruction.side_set, Some(0), "empty FIFO must stall low");
                    x = word >> 31;
                    word <<= 1;
                }
                InstructionOperands::JMP { condition, address } => {
                    let take = match condition {
                        JmpCondition::Always => true,
                        JmpCondition::XIsZero => x == 0,
                        _ => panic!("unexpected branch"),
                    };
                    if take {
                        next = address;
                    }
                }
                InstructionOperands::MOV {
                    destination: MovDestination::Y,
                    op: MovOperation::None,
                    source: MovSource::Y,
                } => {}
                _ => panic!("unexpected instruction"),
            }
            pc = next;
            if pc == program.wrap.target {
                break;
            }
            assert!(total_cycles < 100, "program did not finish a bit");
        }
        bits.push((x, high_cycles, total_cycles));
    }
    bits
}

#[test]
fn grb_channels_and_msb_first_waveform() {
    let color = RGB8 {
        r: 0x12,
        g: 0x34,
        b: 0x56,
    };
    assert_eq!(pack(color), 0x3412_5600);
    // Exercise all byte values in each transmitted channel; padding is not sent.
    for byte in 0..=255u32 {
        for shift in [8, 16, 24] {
            let word = byte << shift;
            for (index, (bit, high, total)) in emitted_bits(word).into_iter().enumerate() {
                assert_eq!(bit, (word >> (31 - index)) & 1);
                assert_eq!(high, if bit == 0 { 5 } else { 10 });
                assert_eq!(total, 20);
            }
        }
    }
}

#[test]
fn pulse_ranges_fit_ws2813b_b_v4_with_fractional_divider_jitter() {
    // Covers both chip defaults and fractional divider cases.
    for sys_hz in [
        32_000_000,
        48_000_000,
        120_000_000,
        123_456_789,
        125_000_000,
        128_000_000,
        133_000_000,
        150_000_000,
    ] {
        let timing = timing(sys_hz);
        let divider = u64::from(timing.divider_256);
        let sys_hz = u64::from(sys_hz);
        for (_, high, total) in emitted_bits(0xaaaa_5555) {
            let bounds_ns = |cycles: u32| {
                let ticks_256 = u64::from(cycles) * divider;
                (
                    (ticks_256 / 256) * 1_000_000_000 / sys_hz,
                    (ticks_256.div_ceil(256) * 1_000_000_000).div_ceil(sys_hz),
                )
            };
            let (min_high, max_high) = bounds_ns(high);
            let (min_low, max_low) = bounds_ns(total - high);
            if high == 5 {
                assert!(min_high >= 220 && max_high <= 380);
                assert!(min_low >= 580 && max_low <= 1000);
            } else {
                assert!(min_high >= 580 && max_high <= 1000);
                assert!(min_low >= 580 && max_low <= 1000);
            }
            assert!(bounds_ns(total).0 >= 1200);
        }
        assert!(divider * 16_000_000 >= sys_hz * 256);
        let worst_tail_ticks = (217 * 20 * divider).div_ceil(256) + 1;
        let remaining_low_ns =
            timing.drain_and_reset_us * 1000 - (worst_tail_ticks * 1_000_000_000).div_ceil(sys_hz);
        assert!(remaining_low_ns >= 350_000);
    }
}

#[test]
#[should_panic(expected = "clk_sys >= 32 MHz")]
fn rejects_clocks_without_timing_guarantees() {
    timing(12_000_000);
}

#[test]
fn padding_byte_is_not_transmitted() {
    assert_eq!(emitted_bits(0x1234_5600), emitted_bits(0x1234_56ff));
    assert_eq!(emitted_bits(0).len(), 24);
}

#[test]
fn default_clocks_allow_full_fifo_drain_plus_reset() {
    for sys_hz in [125_000_000, 150_000_000] {
        assert_eq!(timing(sys_hz).drain_and_reset_us, 622);
    }
}
