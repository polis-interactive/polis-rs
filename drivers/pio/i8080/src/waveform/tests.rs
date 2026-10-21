extern crate std;

use super::*;
use pio::{Instruction, InstructionOperands as I, JmpCondition, OutDestination};
use std::{collections::VecDeque, vec::Vec};

// Execute the actual assembled instructions. Models byte replication on narrow
// FIFO writes, stalls, WR edges, and IRQ ordering; not pad/silicon timing or DMA.
struct Machine {
    program: pio::Program<32>,
    pc: usize,
    x: u32,
    osr: u32,
    isr: u32,
    fifo: VecDeque<u32>,
    rx: VecDeque<u32>,
    data: u8,
    wr: bool,
    cycle: u64,
    data_at: u64,
    fell_at: u64,
    rose_at: Option<u64>,
    bytes: Vec<u8>,
    irqs: usize,
}

impl Machine {
    fn new() -> Self {
        Self {
            program: program(),
            pc: 0,
            x: 0,
            osr: 0,
            isr: 0,
            fifo: VecDeque::new(),
            rx: VecDeque::new(),
            data: 0,
            wr: true,
            cycle: 0,
            data_at: 0,
            fell_at: 0,
            rose_at: None,
            bytes: Vec::new(),
            irqs: 0,
        }
    }

    fn step(&mut self) {
        let i = Instruction::decode(self.program.code[self.pc], self.program.side_set).unwrap();
        let wr = i.side_set.unwrap_or(1) != 0;
        if self.wr && !wr {
            assert!(self.cycle - self.data_at >= 1, "data setup before WR low");
            if let Some(rose) = self.rose_at {
                assert!(self.cycle - rose >= 2);
            }
            self.fell_at = self.cycle;
        }
        if !self.wr && wr {
            assert!(self.cycle - self.fell_at >= 2, "WR low pulse");
            assert!(self.cycle - self.data_at >= 3, "data setup before latch");
            self.rose_at = Some(self.cycle);
            self.bytes.push(self.data);
        }
        self.wr = wr;
        let mut next = if self.pc == self.program.wrap.source as usize {
            self.program.wrap.target as usize
        } else {
            self.pc + 1
        };
        match i.operands {
            I::PUSH {
                if_full: false,
                block: true,
            } => {
                if self.rx.len() == 4 {
                    next = self.pc;
                } else {
                    self.rx.push_back(self.isr);
                    self.isr = 0;
                }
            }
            I::PULL {
                if_empty: false,
                block: true,
            } => {
                if let Some(word) = self.fifo.pop_front() {
                    self.osr = word;
                } else {
                    assert!(self.wr, "starvation must leave WR high");
                    next = self.pc;
                }
            }
            I::OUT {
                destination: OutDestination::X,
                bit_count: 32,
            } => {
                self.x = self.osr;
                self.osr = 0;
            }
            I::OUT {
                destination: OutDestination::PINS,
                bit_count: 8,
            } => {
                assert!(self.wr);
                if let Some(rose) = self.rose_at {
                    assert!(self.cycle - rose >= 2, "data hold after latch");
                }
                self.data = self.osr as u8;
                self.osr >>= 8;
                self.data_at = self.cycle;
            }
            I::OUT {
                destination: OutDestination::NULL,
                bit_count,
            } => {
                self.osr = self.osr.checked_shr(bit_count.into()).unwrap_or(0);
            }
            I::IN {
                source: pio::InSource::OSR,
                bit_count,
            } => {
                self.isr = (self.isr << bit_count) | (self.osr & ((1 << bit_count) - 1));
            }
            I::MOV {
                destination: pio::MovDestination::OSR,
                op: pio::MovOperation::None,
                source: pio::MovSource::ISR,
            } => {
                self.osr = self.isr;
            }
            I::JMP {
                condition: JmpCondition::XDecNonZero,
                address,
            } => {
                let old = self.x;
                self.x = self.x.wrapping_sub(1);
                if old != 0 {
                    next = address as usize;
                }
            }
            I::MOV {
                destination: pio::MovDestination::Y,
                op: pio::MovOperation::None,
                source: pio::MovSource::Y,
            } => {}
            I::IRQ {
                clear: false,
                wait: false,
                index: 0,
                index_mode: pio::IrqIndexMode::REL,
            } => {
                assert!(self.wr);
                assert!(self.cycle - self.rose_at.unwrap() >= 2, "IRQ after hold");
                self.irqs += 1;
            }
            _ => panic!("unexpected instruction: {i:?}"),
        }
        self.pc = next;
        self.cycle += 1 + u64::from(i.delay);
    }

    fn write(&mut self, bytes: &[u8], gaps: bool) {
        assert!(!bytes.is_empty());
        let previous = self.irqs;
        self.fifo.push_back((bytes.len() - 1) as u32);
        for (n, &byte) in bytes.iter().enumerate() {
            if gaps {
                for _ in 0..(n % 13 + 40) {
                    self.step();
                }
                assert_eq!(self.irqs, previous, "no early completion");
            }
            self.fifo.push_back(u32::from_ne_bytes([byte; 4]));
        }
        let bound = self.cycle + bytes.len() as u64 * 30 + 16;
        while self.irqs == previous {
            self.step();
            assert!(self.cycle <= bound, "missing completion");
        }
        assert_eq!(self.irqs, previous + 1);
        for _ in 0..10 {
            self.step();
        }
        assert_eq!(self.irqs, previous + 1, "exactly one IRQ per transfer");
        assert!(self.fifo.is_empty());
    }
}

fn expanded(pixel: u8) -> [u8; 2] {
    let r = u16::from(pixel >> 5);
    let g = u16::from((pixel >> 2) & 7);
    let b = u16::from(pixel & 3);
    (((r << 2 | r >> 1) << 11) | ((g << 3 | g) << 5) | (b << 3 | b << 1 | b >> 1)).to_be_bytes()
}

#[test]
fn rgb332_all_colors_byte_order_timing_and_starvation() {
    assert_eq!(program().code.len() + rgb332_program().code.len(), 31);
    for gaps in [false, true] {
        let mut m = Machine::new();
        m.program = rgb332_program();
        let pixels: Vec<u8> = (0..=255).collect();
        m.write(&pixels, gaps);
        let expected: Vec<u8> = pixels.iter().flat_map(|&p| expanded(p)).collect();
        assert_eq!(m.bytes, expected);
        m.bytes.clear();
        // Reuse ISR across single-pixel transfers and a maximum-size DMA chunk.
        for pixel in [0xff, 0, 0xe0, 0x1c, 3] {
            m.write(&[pixel], true);
        }
        assert_eq!(m.bytes, [0xff, 0xff, 0, 0, 0xf8, 0, 7, 0xe0, 0, 31]);
        m.bytes.clear();
        let pixels: Vec<u8> = (0..65535).map(|n| n as u8).collect();
        m.write(&pixels, gaps);
        m.write(&[0x55, 0xaa], gaps);
        assert_eq!(
            m.bytes,
            pixels
                .iter()
                .chain(&[0x55, 0xaa])
                .flat_map(|&p| expanded(p))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn rgb332_pixel_rate_and_final_hold() {
    let mut m = Machine::new();
    m.program = rgb332_program();
    m.fifo.extend([1, 0x1c1c1c1c, 0xe0e0e0e0]);
    let mut edges = Vec::new();
    while m.irqs == 0 {
        let before = m.bytes.len();
        m.step();
        if m.bytes.len() != before {
            edges.push(m.rose_at.unwrap());
        }
        if m.fifo.is_empty() && m.bytes.len() < 4 {
            assert_eq!(m.irqs, 0);
        }
    }
    assert_eq!(edges[2] - edges[0], 25);
    assert_eq!(edges[3] - edges[1], 25);
    assert_eq!(m.bytes, [7, 0xe0, 0xf8, 0]);
}

#[test]
fn byte_order_pulse_timing_and_completion_with_fifo_starvation() {
    for gaps in [false, true] {
        let mut m = Machine::new();
        for _ in 0..100 {
            m.step();
        }
        assert!(m.bytes.is_empty());
        let bytes: Vec<u8> = (0..=255).collect();
        m.write(&bytes, gaps);
        assert_eq!(m.bytes, bytes);
    }
}

#[test]
fn adjacent_command_parameters_pixels_and_large_counts() {
    let mut m = Machine::new();
    let mut expected = Vec::new();
    for bytes in [&[0x2a][..], &[0, 0, 1, 63][..], &[0x2c][..]] {
        m.write(bytes, true);
        expected.extend_from_slice(bytes);
    }
    let pixels: Vec<u8> = (0..65535).map(|i| (i ^ (i >> 8)) as u8).collect();
    m.write(&pixels, false);
    expected.extend_from_slice(&pixels);
    m.write(&[0xff], false);
    expected.push(0xff);
    assert_eq!(m.bytes, expected);
}

#[test]
fn completion_is_later_than_fifo_empty() {
    let mut m = Machine::new();
    m.fifo.extend([0, 0xa5a5a5a5]);
    while !m.fifo.is_empty() {
        m.step();
    }
    assert_eq!(m.irqs, 0);
    assert!(m.bytes.is_empty());
    while m.irqs == 0 {
        m.step();
    }
    assert_eq!(m.bytes, [0xa5]);
}

#[test]
fn integer_divider_never_exceeds_requested_clock_or_timing_limit() {
    for sys in [12_000_000, 125_000_000, 133_000_000, 150_000_000] {
        for max in [1_000, 12_000_000, 24_000_000, 50_000_000] {
            let divider = crate::Config { max_clock_hz: max }.divider(sys);
            if sys.div_ceil(max) > 65535 {
                assert_eq!(divider, Err(crate::ConfigError::Clock));
                continue;
            }
            let divisor = u64::from(divider.unwrap());
            assert!(u64::from(sys) <= divisor * u64::from(max));
            // One full PIO cycle >=20 ns, two low cycles >=40 ns;
            // six cycles per byte >=120 ns, exceeding ILI9341's 66 ns minimum.
            assert!(divisor * 1_000_000_000 >= u64::from(sys) * 20);
        }
    }
    for max in [0, 50_000_001, u32::MAX] {
        assert_eq!(
            crate::Config { max_clock_hz: max }.divider(125_000_000),
            Err(crate::ConfigError::Clock)
        );
    }
    assert_eq!(
        crate::Config::default().divider(0),
        Err(crate::ConfigError::Clock)
    );
}

#[test]
fn validates_bus_bit_order_and_exclusive_wr() {
    assert!(validate_pins([2, 3, 4, 5, 6, 7, 8, 9], 10).is_ok());
    assert!(validate_pins([2, 3, 4, 5, 6, 7, 8, 9], 5).is_err());
    assert!(validate_pins([2, 3, 4, 5, 6, 7, 9, 8], 10).is_err());
    assert!(validate_pins([29, 30, 31, 0, 1, 2, 3, 4], 10).is_err());
}

#[test]
fn converter_all_colors_with_independent_input_and_output_backpressure() {
    assert_eq!(rgb332_converter_program().code.len(), 14);
    assert_eq!(
        program().code.len() + rgb332_converter_program().code.len(),
        21
    );
    // Fast input/slow output fills RX; slow input/fast output starves TX.
    for (input_period, output_period) in [(1, 1), (1, 97), (53, 1), (29, 67)] {
        let mut m = Machine::new();
        m.program = rgb332_converter_program();
        let input: Vec<u8> = (0..1025).map(|i| i as u8).collect();
        let mut sent = 0;
        let mut wire = Vec::new();
        for tick in 0..200_000 {
            if tick % input_period == 0 && sent < input.len() && m.fifo.len() < 4 {
                m.fifo.push_back(u32::from_le_bytes([input[sent]; 4]));
                sent += 1;
            }
            m.step();
            if tick % output_period == 0
                && let Some(word) = m.rx.pop_front()
            {
                // Halfword RX DMA with BSWAP, stored to little-endian RAM.
                wire.extend_from_slice(&(word as u16).swap_bytes().to_le_bytes());
            }
            if wire.len() == input.len() * 2 {
                break;
            }
        }
        assert_eq!(
            wire,
            input.iter().flat_map(|&p| expanded(p)).collect::<Vec<_>>()
        );
        assert_eq!(sent, input.len());
        for _ in 0..100 {
            m.step();
        }
        assert!(m.rx.is_empty(), "no duplicate output after the last input");
        assert!(m.bytes.is_empty(), "converter does not drive LCD pins");
        assert_eq!(m.irqs, 0);
        // Feed staged bytes to the actual raw output program, including an odd
        // final pixel. WR setup/hold assertions run on every byte.
        let mut output = Machine::new();
        for chunk in wire.chunks(514) {
            output.write(chunk, true);
        }
        assert_eq!(output.bytes, wire);
    }
}

#[test]
fn converter_costs_fourteen_cycles_per_pixel_without_stalls() {
    let mut m = Machine::new();
    m.program = rgb332_converter_program();
    m.fifo.extend([0xe0e0e0e0, 0x1c1c1c1c, 0x03030303]);
    for expected in [0xf800, 0x07e0, 0x001f] {
        let start = m.cycle;
        while m.rx.is_empty() {
            m.step();
        }
        assert_eq!(m.cycle - start, 14);
        assert_eq!(m.rx.pop_front(), Some(expected));
    }
}
