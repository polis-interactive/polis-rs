use super::*;
use std::{collections::VecDeque, vec::Vec};

/// Execute the emitted PIO v0 instruction subset with the driver's actual
/// shift/FIFO settings. Edges record physical SCL after target clock stretching.
pub(crate) struct Machine {
    pub program: pio::Program<32>,
    pc: usize,
    osr: u32,
    shifted: u8,
    isr: u32,
    x: u32,
    tx: VecDeque<u32>,
    pub rx: Vec<u32>,
    pub incoming: VecDeque<bool>,
    pub edges: Vec<(u64, bool, bool)>,
    pub sda_edges: Vec<(u64, bool, bool)>,
    pub cycles: u64,
    pub scl: bool,
    pub sda: bool,
    scl_released: bool,
    stretch_left: u32,
    pub stretch: u32,
    pub waits: Vec<u8>,
}

impl Machine {
    pub fn new(offset: u8) -> Self {
        let program = program(offset);
        Self {
            pc: program.wrap.target as usize,
            program,
            osr: 0,
            shifted: 32,
            isr: 0,
            x: 0,
            tx: VecDeque::new(),
            rx: Vec::new(),
            incoming: VecDeque::new(),
            edges: Vec::new(),
            sda_edges: Vec::new(),
            cycles: 0,
            scl: true,
            sda: true,
            scl_released: true,
            stretch_left: 0,
            stretch: 0,
            waits: Vec::new(),
        }
    }

    fn scl_release(&mut self, release: bool) {
        if release && !self.scl_released {
            self.stretch_left = self.stretch;
        }
        self.scl_released = release;
        self.update_scl();
    }

    fn update_scl(&mut self) {
        let high = self.scl_released && self.stretch_left == 0;
        if high != self.scl {
            self.scl = high;
            self.edges.push((self.cycles, high, self.sda));
        }
    }

    fn set_sda(&mut self, high: bool) {
        if high != self.sda {
            self.sda = high;
            self.sda_edges.push((self.cycles, self.scl, high));
        }
    }

    fn tick(&mut self, cycles: u32) {
        for _ in 0..cycles {
            self.cycles += 1;
            self.stretch_left = self.stretch_left.saturating_sub(1);
            self.update_scl();
        }
    }

    fn execute(&mut self, word: u16) {
        if word & 0x1000 != 0 {
            self.scl_release(word & 0x0800 == 0);
        }
        let delay = u32::from((word >> 8) & 7);
        let arg = word & 0xff;
        let mut exec = None;
        match word >> 13 {
            0 => {
                let branch = match arg >> 5 {
                    0 => true,
                    1 => self.x == 0,
                    2 => {
                        let nonzero = self.x != 0;
                        self.x = self.x.wrapping_sub(1);
                        nonzero
                    }
                    _ => panic!("unsupported branch"),
                };
                if branch {
                    self.pc = usize::from(arg & 31);
                }
            }
            1 => {
                assert_eq!(arg >> 5, 5); // WAIT 1 PIN
                let offset = (arg & 31) as u8;
                self.waits.push(offset);
                if offset == 0 {
                    assert!(self.sda);
                } else {
                    while !self.scl {
                        self.tick(1);
                    }
                }
            }
            2 => {
                assert_eq!(arg, 1); // IN PINS, 1
                let input = self.incoming.pop_front().expect("missing sampled bit");
                self.isr = (self.isr << 1) | u32::from(input);
            }
            3 => {
                if self.shifted >= 16 {
                    self.osr = self.tx.pop_front().expect("unexpected TX underflow");
                    self.shifted = 0;
                }
                let count = if arg & 31 == 0 { 32 } else { (arg & 31) as u8 };
                let value = self.osr >> (32 - count);
                self.osr = self.osr.checked_shl(u32::from(count)).unwrap_or(0);
                self.shifted += count;
                match arg >> 5 {
                    1 => self.x = value,
                    3 => {}
                    4 => self.set_sda(value == 0),
                    7 => exec = Some(value as u16),
                    _ => panic!("unsupported OUT"),
                }
            }
            4 => {
                assert_eq!(arg, 0x20);
                self.rx.push(self.isr);
                self.isr = 0;
            }
            5 => match arg {
                0xc3 => self.isr = 0, // MOV ISR, NULL
                0x42 => {}            // NOP = MOV Y, Y
                _ => panic!("unsupported MOV {arg:x}"),
            },
            7 => match arg >> 5 {
                1 => self.x = u32::from(arg & 31),
                4 => self.set_sda(arg & 1 == 0),
                _ => panic!("unsupported SET"),
            },
            _ => panic!("unsupported instruction {word:x}"),
        }
        self.tick(1 + delay);
        if let Some(word) = exec {
            self.execute(word);
        }
    }

    pub fn exchange(&mut self, words: &[u16]) -> u32 {
        self.tx.extend(words.iter().map(|&w| fifo_word(w)));
        let before = self.rx.len();
        for _ in 0..10_000 {
            let word = self.program.code[self.pc];
            self.pc = if self.pc == usize::from(self.program.wrap.source) {
                usize::from(self.program.wrap.target)
            } else {
                self.pc + 1
            };
            self.execute(word);
            if self.rx.len() > before {
                return *self.rx.last().unwrap();
            }
        }
        panic!("PIO did not complete");
    }

    pub fn samples(&mut self, data: u8, nack: bool) {
        self.incoming
            .extend((0..8).rev().map(|i| data & (1 << i) != 0));
        self.incoming.push_back(nack);
    }
}

#[test]
fn all_bytes_are_msb_first_open_drain_and_return_the_ninth_bit() {
    for offset in [1, 31, 12] {
        for value in 0..=255 {
            for nack in [false, true] {
                let mut m = Machine::new(offset);
                m.scl_release(false);
                m.samples(value, nack);
                assert_eq!(
                    m.exchange(&[byte(value, true)]),
                    (u32::from(value) << 1) | u32::from(nack)
                );
                let highs: Vec<_> = m.edges.iter().filter(|e| e.1).collect();
                assert_eq!(highs.len(), 9);
                for (bit, edge) in highs[..8].iter().enumerate() {
                    assert_eq!(edge.2, value & (1 << (7 - bit)) != 0);
                }
                assert!(highs[8].2, "write releases SDA for target ACK");
                assert!(!m.scl, "completion follows the ACK falling edge");
                assert_eq!(m.waits, [offset; 9]);
            }
        }
    }
}

#[test]
fn reads_release_data_and_drive_only_the_requested_ack() {
    let mut m = Machine::new(31);
    m.scl_release(false);
    for nack in [false, true] {
        m.samples(0xa5, nack);
        let start = m.edges.len();
        assert_eq!(m.exchange(&[byte(0xff, nack)]) >> 1, 0xa5);
        let highs: Vec<_> = m.edges[start..].iter().filter(|e| e.1).collect();
        assert!(highs[..8].iter().all(|e| e.2));
        assert_eq!(highs[8].2, nack);
    }
}

#[test]
fn pulses_meet_standard_and_fast_mode_even_with_stretch_and_divider_jitter() {
    for sys in [32_000_000, 125_000_000, 133_000_000, 150_000_000] {
        for (hz, min_low_ns, min_high_ns) in [(100_000, 4700, 4000), (400_000, 1300, 600)] {
            let div = u64::from(divider(sys, hz).unwrap());
            for stretch in [0, 3, 100] {
                let mut m = Machine::new(31);
                m.stretch = stretch;
                m.scl_release(false);
                m.samples(0x55, false);
                m.exchange(&[byte(0x55, true)]);
                for pair in m.edges.windows(2) {
                    let cycles = pair[1].0 - pair[0].0;
                    // Exclude the initial software-driven low before entering byte loop.
                    if pair[0].0 == 0 {
                        continue;
                    }
                    let shortest_ticks = cycles * div / 256;
                    let ns = shortest_ticks * 1_000_000_000 / u64::from(sys);
                    assert!(
                        ns >= if pair[0].1 { min_high_ns } else { min_low_ns },
                        "{sys} Hz sys, {hz} Hz SCL, stretch {stretch}: {pair:?}, {ns} ns"
                    );
                }
            }
            assert!(u64::from(sys) * 256 <= u64::from(hz) * CYCLES_PER_BIT * div);
        }
    }
}

#[test]
fn pin_offsets_divider_limits_and_shared_instruction_budget() {
    assert_eq!(scl_offset(6, 5), 31);
    assert_eq!(scl_offset(6, 7), 1);
    for sda in 0..30 {
        for scl in 0..30 {
            if sda != scl {
                assert_eq!((sda + scl_offset(sda, scl)) % 32, scl);
            }
        }
    }
    assert!(divider(125_000_000, 0).is_none());
    assert!(divider(125_000_000, 400_001).is_none());
    assert!(divider(125_000_000, 1).is_none());
    assert!(divider(31_999_999, 100_000).is_none());
    assert_eq!(program(31).code.len(), 20);
    assert!(program(31).code.len() + 4 <= 32, "coexists with SK6812");
}
