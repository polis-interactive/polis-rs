extern crate std;

use super::*;
use crate::{Direction, Segment, StopReason, queue::Queue};
use pio::{
    Instruction, InstructionOperands as I, JmpCondition as J, MovDestination as D,
    MovOperation as O, MovSource as S, OutDestination, SetDestination,
};
use std::{collections::VecDeque, vec::Vec};

// Cycle-level interpreter of the ACTUAL assembled PIO v0 program. Models delay
// cycles, optional side-set, right OUT, nonblocking PULL fallback, scratch
// counters, four-word FIFOs and IRQ. It is not a model of DMA/APB/pad latency.
struct Machine {
    code: pio::Program<32>,
    pc: u8,
    delay: u8,
    x: u32,
    y: u32,
    osr: u32,
    isr: u32,
    tx: VecDeque<u32>,
    rx: VecDeque<u32>,
    step: bool,
    dir: bool,
    cycle: u64,
    rises: Vec<(u64, bool)>,
    falls: Vec<u64>,
    dirs: Vec<(u64, bool)>,
    irq: bool,
}

impl Machine {
    fn new(config: Config) -> Self {
        Self {
            code: program(config),
            pc: START,
            delay: 0,
            x: 0xdeadbeef,
            y: 0xabcdef01,
            osr: 0,
            isr: 0,
            tx: VecDeque::new(),
            rx: VecDeque::new(),
            step: false,
            dir: false,
            cycle: 0,
            rises: Vec::new(),
            falls: Vec::new(),
            dirs: Vec::new(),
            irq: false,
        }
    }

    fn tick(&mut self) {
        if self.delay > 0 {
            self.delay -= 1;
            self.cycle += 1;
            return;
        }
        let i =
            Instruction::decode(self.code.code[usize::from(self.pc)], self.code.side_set).unwrap();
        if let Some(level) = i.side_set {
            let next_step = level != 0;
            if next_step && !self.step {
                self.rises.push((self.cycle, self.dir));
            }
            if !next_step && self.step {
                self.falls.push(self.cycle);
            }
            self.step = next_step;
        }
        let mut next = if self.pc == self.code.wrap.source {
            self.code.wrap.target
        } else {
            self.pc + 1
        };
        match i.operands {
            I::SET {
                destination: SetDestination::X,
                data,
            } => self.x = u32::from(data),
            I::PULL {
                if_empty: false,
                block: false,
            } => self.osr = self.tx.pop_front().unwrap_or(self.x),
            I::OUT {
                destination,
                bit_count,
            } => {
                let mask = (1u32 << bit_count) - 1;
                let value = self.osr & mask;
                self.osr >>= bit_count;
                match destination {
                    OutDestination::Y => self.y = value,
                    OutDestination::ISR => self.isr = value,
                    OutDestination::PINS => {
                        let dir = value != 0;
                        if dir != self.dir {
                            self.dirs.push((self.cycle, dir));
                        }
                        self.dir = dir;
                    }
                    _ => panic!("unexpected OUT"),
                }
            }
            I::MOV {
                destination,
                op: O::None,
                source,
            } => {
                let value = match source {
                    S::OSR => self.osr,
                    S::ISR => self.isr,
                    S::Y => self.y,
                    _ => panic!("unexpected MOV"),
                };
                match destination {
                    D::X => self.x = value,
                    D::Y => self.y = value,
                    D::ISR => self.isr = value,
                    _ => panic!("unexpected MOV"),
                }
            }
            I::JMP { condition, address } => {
                let branch = match condition {
                    J::Always => true,
                    J::XIsZero => self.x == 0,
                    J::YIsZero => self.y == 0,
                    J::XDecNonZero => {
                        let old = self.x;
                        self.x = self.x.wrapping_sub(1);
                        old != 0
                    }
                    J::YDecNonZero => {
                        let old = self.y;
                        self.y = self.y.wrapping_sub(1);
                        old != 0
                    }
                    _ => panic!("unexpected JMP"),
                };
                if branch {
                    next = address;
                }
            }
            I::PUSH {
                if_full: false,
                block: true,
            } => {
                assert!(
                    self.rx.len() < 4,
                    "completion FIFO overflow would stall execution"
                );
                self.rx.push_back(self.isr);
                self.isr = 0;
            }
            I::IRQ {
                clear: false,
                wait: false,
                ..
            } => self.irq = true,
            _ => panic!("unexpected instruction {:?}", i),
        }
        self.pc = next;
        self.delay = i.delay;
        self.cycle += 1;
    }

    fn load(&mut self, queue: &mut Queue, c: Config, segments: &[Segment]) {
        for &segment in segments {
            let e = c.encode(segment, queue.direction).unwrap();
            queue.push(e, c.max_committed_ticks).unwrap();
            assert!(self.tx.len() < 4);
            self.tx.push_back(e.word);
        }
    }

    fn terminal(&mut self) {
        for _ in 0..3_000_000 {
            if self.irq {
                return;
            }
            self.tick();
        }
        panic!("did not terminate");
    }
}

fn config() -> Config {
    Config {
        tick_hz: 4_000_000,
        step_high_ticks: 4,
        dir_hold_ticks: 4,
        dir_setup_ticks: 4,
        max_committed_ticks: 10_000_000,
    }
}

fn pulses(direction: Direction, count: u16, period_ticks: u32) -> Segment {
    Segment::Pulses {
        direction,
        pulses: count,
        period_ticks,
    }
}

#[test]
fn speed_changes_and_reversals_have_exact_edge_spacing() {
    let c = config();
    let mut m = Machine::new(c);
    let mut q = Queue::new(Direction::Negative);
    m.load(
        &mut q,
        c,
        &[
            pulses(Direction::Positive, 2, 80),
            pulses(Direction::Positive, 3, 100),
            pulses(Direction::Negative, 2, 140),
            Segment::End,
        ],
    );
    m.terminal();
    let spacings: Vec<_> = m
        .rises
        .windows(2)
        .map(|pair| pair[1].0 - pair[0].0)
        .collect();
    assert_eq!(spacings, [80, 100, 100, 100, 140, 140]);
    assert_eq!(m.rises[0].0, 17 + 80 - 4 - 4 - 4);
    for (rise, fall) in m.rises.iter().zip(&m.falls) {
        assert_eq!(*fall - rise.0, 4);
    }
    assert_eq!(
        m.rises
            .iter()
            .map(|edge| if edge.1 { 1 } else { -1 })
            .sum::<i32>(),
        3
    );
    for &(time, dir) in &m.dirs {
        if let Some(fall) = m.falls.iter().copied().rfind(|&t| t <= time) {
            assert!(time - fall >= u64::from(c.dir_hold_ticks));
        }
        if let Some(&(rise, _)) = m.rises.iter().find(|&&(t, d)| t >= time && d == dir) {
            assert!(rise - time >= u64::from(c.dir_setup_ticks));
        }
    }
    assert!(!m.step);
    assert!(m.pc == FINISHED || m.pc == FINISHED + 1);
}

#[test]
fn abort_accounting_matches_observed_edges_at_every_cycle() {
    for high in [2, 4, 17] {
        let c = Config {
            step_high_ticks: high,
            ..config()
        };
        let mut m = Machine::new(c);
        let mut q = Queue::new(Direction::Negative);
        m.load(
            &mut q,
            c,
            &[
                pulses(Direction::Positive, 3, 80),
                Segment::Delay { ticks: 100 },
                pulses(Direction::Negative, 2, 100),
                Segment::End,
            ],
        );
        loop {
            // Match driver's disabled-SM snapshot: drain completed records first.
            let mut snapshot_queue = q.clone();
            for _ in &m.rx {
                snapshot_queue.acknowledge();
            }
            let report = snapshot_queue.stopped(m.pc, m.y, StopReason::Aborted);
            assert_eq!(
                report.emitted_pulses,
                m.rises.len() as u64,
                "PC={}, cycle={}",
                m.pc,
                m.cycle
            );
            assert_eq!(
                report.emitted_signed_pulses,
                m.rises
                    .iter()
                    .map(|edge| if edge.1 { 1 } else { -1 })
                    .sum::<i64>()
            );
            if m.step {
                assert!(
                    report.pulse_may_be_truncated,
                    "unreported high pulse at PC {}",
                    m.pc
                );
            }
            if m.irq {
                break;
            }
            m.tick();
        }
    }
}

#[test]
fn minimum_period_high_hold_and_setup_ranges() {
    for high in [2, 4, 17] {
        for hold in [1, 4, 16] {
            for setup in [0, 30] {
                let c = Config {
                    step_high_ticks: high,
                    dir_hold_ticks: hold,
                    dir_setup_ticks: setup,
                    ..config()
                };
                let period = c.min_period_ticks();
                let mut m = Machine::new(c);
                m.load(
                    &mut Queue::new(Direction::Negative),
                    c,
                    &[
                        pulses(Direction::Positive, 3, period),
                        pulses(Direction::Negative, 1, period),
                        Segment::End,
                    ],
                );
                m.terminal();
                assert_eq!(m.rises.len(), 4);
                for pair in m.rises.windows(2) {
                    assert_eq!(pair[1].0 - pair[0].0, u64::from(period));
                }
                for (rise, fall) in m.rises.iter().zip(&m.falls) {
                    assert_eq!(*fall - rise.0, u64::from(high));
                }
                for &(time, _) in &m.dirs {
                    let rise = m.rises.iter().find(|&&(t, _)| t >= time).unwrap().0;
                    assert!(rise - time >= u64::from(setup));
                }
            }
        }
    }
}

#[test]
fn delays_extend_low_time_without_edges_or_phase_loss() {
    let c = config();
    let mut m = Machine::new(c);
    m.load(
        &mut Queue::new(Direction::Negative),
        c,
        &[
            pulses(Direction::Positive, 1, 80),
            Segment::Delay { ticks: 1000 },
            pulses(Direction::Positive, 1, 120),
            Segment::End,
        ],
    );
    m.terminal();
    assert_eq!(m.rises.len(), 2);
    assert_eq!(m.rises[1].0 - m.rises[0].0, 1120);
}

#[test]
fn starvation_and_end_ignore_late_input_even_if_irq_is_cleared() {
    let c = config();
    for end in [false, true] {
        let mut m = Machine::new(c);
        let mut q = Queue::new(Direction::Negative);
        m.load(&mut q, c, &[pulses(Direction::Positive, 1, 80)]);
        if end {
            m.load(&mut q, c, &[Segment::End]);
        }
        m.terminal();
        assert!(!m.step);
        assert_eq!(m.pc, if end { FINISHED + 1 } else { STARVED + 1 });
        m.tx.push_back(
            c.encode(pulses(Direction::Positive, 20, 80), Direction::Positive)
                .unwrap()
                .word,
        );
        for _ in 0..1000 {
            m.irq = false;
            m.tick();
        }
        assert_eq!(m.rises.len(), 1);
        assert!(!m.step);
    }
}

#[test]
fn empty_move_one_long_and_maximum_period_segments() {
    let c = config();
    for (count, period) in [
        (0, 80),
        (1, c.max_period_ticks()),
        (u16::MAX, c.min_period_ticks()),
    ] {
        let mut m = Machine::new(c);
        let mut q = Queue::new(Direction::Negative);
        if count > 0 {
            m.load(&mut q, c, &[pulses(Direction::Negative, count, period)]);
        }
        m.load(&mut q, c, &[Segment::End]);
        m.terminal();
        assert_eq!(m.rises.len(), usize::from(count));
        assert_eq!(m.falls.len(), usize::from(count));
    }
}

#[test]
fn refill_after_completion_preserves_continuity_and_fifo_capacity() {
    let c = config();
    let mut m = Machine::new(c);
    let mut q = Queue::new(Direction::Negative);
    for _ in 0..4 {
        m.load(&mut q, c, &[pulses(Direction::Positive, 1, 80)]);
    }
    let mut submitted = 4;
    while !m.irq {
        m.tick();
        while m.rx.pop_front().is_some() {
            q.acknowledge();
            if submitted < 100 {
                m.load(&mut q, c, &[pulses(Direction::Positive, 1, 80)]);
                submitted += 1;
            } else if !q.closed {
                m.load(&mut q, c, &[Segment::End]);
            }
        }
    }
    assert_eq!(m.rises.len(), 100);
    assert_eq!(q.progress().completed_pulses, 100);
    assert!(m.rises.windows(2).all(|pair| pair[1].0 - pair[0].0 == 80));
}

#[test]
fn cancellation_accounts_for_every_dma_prefix_and_execution_phase() {
    let c = config();
    let segments = [
        pulses(Direction::Positive, 2, 80),
        Segment::Delay { ticks: 40 },
        pulses(Direction::Negative, 3, 90),
        Segment::End,
    ];
    let (q, words) = Queue::new(Direction::Negative)
        .prepare(c, &segments)
        .unwrap();
    // Admission precedes DMA. Any prefix can have reached the FIFO when its
    // future is dropped. Unsupplied commands must never count as emitted.
    for prefix in 0..=words.len() {
        let mut m = Machine::new(c);
        m.tx.extend(&words[..prefix]);
        loop {
            let mut snapshot = q.clone();
            for _ in &m.rx {
                snapshot.acknowledge();
            }
            let report = snapshot.stopped(m.pc, m.y, StopReason::Cancelled);
            assert_eq!(report.emitted_pulses, m.rises.len() as u64);
            assert_eq!(
                report.emitted_signed_pulses,
                m.rises
                    .iter()
                    .map(|&(_, dir)| if dir { 1 } else { -1 })
                    .sum::<i64>()
            );
            if m.irq {
                break;
            }
            m.tick();
        }
    }
}
