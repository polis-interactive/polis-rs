extern crate std;

use super::*;
use crate::{Decoder, Direction, Phase, Position, Resolution, Tracker};
use pio::{
    InSource, Instruction, InstructionOperands as I, JmpCondition as J, MovDestination as D,
    MovOperation as O, MovSource as S, OutDestination, SetDestination,
};
use std::collections::VecDeque;

// Instruction-level interpreter for the actual assembled words loaded by HAL.
// Implements only the used PIO v0 instructions and configured left IN/right OUT,
// no autopush/pull, eight-word joined RX. Unsupported instructions fail closed.
// Not a simulation of pads, metastability, APB, IRQ delivery, or silicon timing.
struct Machine {
    program: pio::Program<32>,
    pc: u8,
    y: u32,
    isr: u32,
    osr: u32,
    pins: u32,
    fifo: VecDeque<u32>,
    dropped: usize,
    cycles: u64,
    sampled: bool,
}

impl Machine {
    fn new(phase: u8) -> Self {
        Self {
            program: program(),
            pc: START,
            y: 0x12345678,
            isr: 0xabcdef,
            osr: 0x12345678,
            pins: 0xffff_fffc | u32::from(phase),
            fifo: VecDeque::new(),
            dropped: 0,
            cycles: 0,
            sampled: false,
        }
    }

    fn tick(&mut self) {
        let instruction =
            Instruction::decode(self.program.code[self.pc as usize], self.program.side_set)
                .unwrap();
        assert_eq!(instruction.delay, 0);
        assert_eq!(instruction.side_set, None);
        let mut next = if self.pc == self.program.wrap.source {
            self.program.wrap.target
        } else {
            self.pc + 1
        };
        self.sampled = false;
        match instruction.operands {
            I::JMP { condition, address } => {
                let take = match condition {
                    J::Always => true,
                    J::YDecNonZero => {
                        let old = self.y;
                        self.y = self.y.wrapping_sub(1);
                        old != 0
                    }
                    _ => panic!("unsupported JMP"),
                };
                if take {
                    next = address;
                }
            }
            I::MOV {
                destination,
                op,
                source,
            } => {
                let value = match source {
                    S::Y => self.y,
                    S::ISR => self.isr,
                    S::PINS => self.pins,
                    _ => panic!("unsupported MOV source"),
                };
                let value = match op {
                    O::None => value,
                    O::Invert => !value,
                    _ => panic!("unsupported MOV operation"),
                };
                match destination {
                    D::Y => self.y = value,
                    D::ISR => self.isr = value,
                    D::OSR => self.osr = value,
                    D::PC => next = (value & 31) as u8,
                    _ => panic!("unsupported MOV destination"),
                }
            }
            I::PUSH {
                if_full: false,
                block: false,
            } => {
                if self.fifo.len() < 8 {
                    self.fifo.push_back(self.isr);
                } else {
                    self.dropped += 1;
                }
                self.isr = 0;
            }
            I::OUT {
                destination: OutDestination::ISR,
                bit_count: 2,
            } => {
                self.isr = self.osr & 3;
                self.osr >>= 2;
            }
            I::IN {
                source: InSource::PINS,
                bit_count: 2,
            } => {
                self.isr = (self.isr << 2) | (self.pins & 3);
                self.sampled = true;
            }
            I::SET {
                destination: SetDestination::Y,
                data: 0,
            } => self.y = 0,
            _ => panic!("unsupported instruction: {:?}", instruction),
        }
        self.pc = next;
        self.cycles += 1;
    }

    fn boot(&mut self) {
        for _ in 0..3 {
            self.tick();
        }
        assert_eq!(self.pc, 15);
        assert_eq!(self.y, 0);
    }

    fn phase(&mut self, phase: u8) -> u32 {
        self.pins = 0xffff_fffc | u32::from(phase);
        let start = self.cycles;
        // One whole sample/dispatch/count iteration, including table entry 15.
        loop {
            self.tick();
            if self.pc == 15 {
                break;
            }
            assert!(self.cycles - start <= u64::from(MAX_SAMPLE_CYCLES));
        }
        assert!(self.cycles - start <= u64::from(MAX_SAMPLE_CYCLES));
        self.y
    }

    fn snapshot(&mut self) -> u32 {
        let n = self.fifo.len();
        for _ in 0..n {
            self.fifo.pop_front().unwrap();
        }
        for _ in 0..=MAX_SAMPLE_CYCLES {
            if let Some(value) = self.fifo.pop_front() {
                return value;
            }
            self.tick();
        }
        panic!("no snapshot within the loop bound")
    }
}

fn phase(bits: u8) -> Phase {
    Phase::from_b_a(bits & 1 != 0, bits & 2 != 0)
}

#[test]
fn emitted_program_layout_and_startup_at_all_phases() {
    let p = program();
    assert_eq!(p.code.len(), 27);
    assert_eq!(p.origin, Some(0));
    assert_eq!((p.wrap.target, p.wrap.source), (15, 23));
    for initial in 0..4 {
        let mut m = Machine::new(initial);
        m.boot();
        for _ in 0..20 {
            assert_eq!(m.phase(initial), 0);
        }
        assert_eq!(m.snapshot(), 0);
    }
}

#[test]
fn actual_instructions_match_model_for_every_seven_phase_trace() {
    // 4^7 = 16,384 traces: all starting phases and all six following samples.
    // Includes clean motion, adjacent bounce, reversals, and two-bit jumps.
    for trace in 0..(1u32 << 14) {
        let initial = (trace & 3) as u8;
        let mut m = Machine::new(initial);
        m.boot();
        let mut d = Decoder::default();
        d.sample(phase(initial));
        for index in 1..7 {
            let input = ((trace >> (2 * index)) & 3) as u8;
            assert_eq!(
                m.phase(input),
                d.sample(phase(input)).0,
                "trace {trace:x}, sample {index}"
            );
        }
    }
}

#[test]
fn all_branches_wrap_correctly_at_counter_boundaries() {
    for initial in 0..4 {
        for next in 0..4 {
            for count in [0, 1, u32::MAX, u32::MAX - 1, i32::MAX as u32, 1 << 31] {
                let mut m = Machine::new(initial);
                m.boot();
                m.y = count;
                let mut d = Decoder::default();
                d.sample(phase(initial));
                assert_eq!(m.phase(next), count.wrapping_add(d.sample(phase(next)).0));
            }
        }
    }
}

#[test]
fn full_fifo_does_not_stall_and_delayed_snapshot_recovers_position() {
    let mut m = Machine::new(0);
    m.boot();
    for index in 1..=100_000 {
        m.phase([0, 2, 3, 1][index % 4]);
    }
    assert_eq!(m.y, 100_000);
    assert_eq!(m.fifo.len(), 8);
    assert!(m.dropped > 99_000);
    let raw = Position(m.snapshot());
    assert_eq!(raw, Position(100_000));
    let mut tracker = Tracker::new(Position(0), Resolution::FullStep, Direction::ALeadsB);
    assert_eq!(tracker.update(raw).unwrap().position, 25_000);
    // Snapshot left the PC part-way through update; finish without changing pins.
    while m.pc != 15 {
        m.tick();
    }
    for index in 1..=100_000 {
        m.phase([0, 1, 3, 2][index % 4]);
    }
    assert_eq!(tracker.update(Position(m.snapshot())).unwrap().position, 0);
}

#[test]
fn asynchronous_pin_changes_respect_ten_cycle_sample_bound() {
    let mut m = Machine::new(3);
    m.boot();
    let mut d = Decoder::default();
    d.sample(phase(3));
    let mut last_sample = None;
    let mut expected = 0;
    let mut random = 0x31415926u32;
    for _ in 0..100_000 {
        // Faster-than-sampling input changes deliberately included. Compare to
        // *observed* phases, not unobservable physical travel.
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        m.pins = random;
        m.tick();
        if m.sampled {
            expected = d.sample(phase((m.pins & 3) as u8)).0;
            if let Some(previous) = last_sample {
                assert!(m.cycles - previous <= u64::from(MAX_SAMPLE_CYCLES));
            }
            last_sample = Some(m.cycles);
        }
        if m.pc == 15 {
            assert_eq!(m.y, expected);
        }
    }
}

#[test]
fn sampling_alias_can_hide_whole_cycles_without_diagnostic() {
    let mut m = Machine::new(0);
    m.boot();
    // Physical 00->10->11->01->00 entirely between INs is indistinguishable
    // from no movement. Tests explicitly avoid claiming exact loss detection.
    for bits in [2, 3, 1, 0] {
        m.pins = bits;
    }
    assert_eq!(m.phase(0), 0);
}
