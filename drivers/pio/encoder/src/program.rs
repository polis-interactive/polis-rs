// Adapted from raspberrypi/pico-examples, quadrature_encoder.pio.
// Copyright (c) 2023 Raspberry Pi (Trading) Ltd.
// SPDX-License-Identifier: BSD-3-Clause
// See PROVENANCE.md for the pinned source and local changes.

pub(crate) const START: u8 = 24;
pub(crate) const MAX_SAMPLE_CYCLES: u32 = 10;

pub(crate) fn program() -> pio::Program<32> {
    use pio::{
        InSource, JmpCondition as J, MovDestination as D, MovOperation as O, MovSource as S,
        OutDestination, SetDestination,
    };
    let mut a = pio::Assembler::<32>::new();
    let mut update = a.label();
    let mut decrement = a.label();
    let mut increment = a.label();
    let mut increment_cont = a.label();
    let mut wrap_end = a.label();

    // Index = (previous_AB << 2) | current_AB. B is the low input bit.
    // All four two-bit transitions resynchronize with no count.
    for action in [0, -1, 1, 0, 1, 0, 0, -1, -1, 0, 0, 1, 0, 1] {
        let target = match action {
            -1 => &mut decrement,
            1 => &mut increment,
            _ => &mut update,
        };
        a.jmp(J::Always, target);
    }
    a.bind(&mut decrement); // 14 doubles as table entry 11 -> 10.
    a.jmp(J::YDecNonZero, &mut update); // Both outcomes reach 15; even zero decrements.
    a.bind(&mut update); // 15 doubles as table entry 11 -> 11.
    a.mov(D::ISR, O::None, S::Y);
    a.push(false, false); // Never stall the counter when RX is full.
    a.out(OutDestination::ISR, 2); // Zero-extended previous phase from OSR.
    a.r#in(InSource::PINS, 2); // Left shift: previous_AB, current_AB.
    a.mov(D::OSR, O::None, S::ISR);
    a.mov(D::PC, O::None, S::ISR);
    a.bind(&mut increment); // 21
    a.mov(D::Y, O::Invert, S::Y);
    a.jmp(J::YDecNonZero, &mut increment_cont);
    a.bind(&mut increment_cont);
    a.mov(D::Y, O::Invert, S::Y);
    a.bind(&mut wrap_end); // Exclusive end: instruction 23 wraps to update.

    // Enter here once, with synchronized input levels. Never assume phase 00.
    a.mov(D::OSR, O::None, S::PINS); // 24: low two bits are initial AB.
    a.set(SetDestination::Y, 0);
    a.jmp(J::Always, &mut update);
    a.assemble_with_wrap(wrap_end, update).set_origin(Some(0))
}

#[cfg(test)]
mod tests;
