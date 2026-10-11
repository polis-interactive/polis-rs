// Adapted from pico-examples/pio/i2c/i2c.pio.
// Copyright (c) 2020 Raspberry Pi (Trading) Ltd.
// SPDX-License-Identifier: BSD-3-Clause

use pio::{
    Assembler, InSource, JmpCondition, MovDestination, MovOperation, MovSource, OutDestination,
    SetDestination, SideSet, WaitSource,
};

pub(crate) const CYCLES_PER_BIT: u64 = 36;
const SIDE: SideSet = SideSet::new(true, 1, true);

/// WAIT PIN is relative to IN_BASE and wraps at 32 on RP2040/RP2350A.
pub(crate) fn scl_offset(sda: u8, scl: u8) -> u8 {
    assert!(
        sda < 30 && scl < 30 && sda != scl,
        "invalid RP2040/RP2350A I2C pins"
    );
    scl.wrapping_sub(sda) & 31
}

/// Twenty shared instructions. Output latches stay low: PINDIRS=1 pulls low,
/// PINDIRS=0 releases. Invert data in software instead of upstream's OE override.
/// Unlike upstream, return the sampled byte AND ACK after lowering SCL.
/// This reports every NACK and needs no dedicated PIO IRQ flag.
pub(crate) fn program(scl_offset: u8) -> pio::Program<32> {
    let mut a = Assembler::<32>::new_with_side_set(SIDE);
    let mut byte = a.label();
    let mut bits = a.label();
    let mut entry = a.label();
    let mut exec = a.label();
    let mut end = a.label();
    a.bind(&mut byte);
    a.set(SetDestination::X, 7);
    a.bind(&mut bits);
    // Twenty low cycles, sixteen high cycles: Fast-mode tLOW >= 1.3 us.
    a.nop_with_delay(3);
    a.out_with_delay(OutDestination::PINDIRS, 1, 7);
    a.nop_with_side_set(0);
    a.wait_with_delay(1, WaitSource::PIN, scl_offset, false, 6);
    a.in_with_delay(InSource::PINS, 1, 7);
    a.jmp_with_delay_and_side_set(JmpCondition::XDecNonZero, &mut bits, 7, 1);
    // Ninth clock: release SDA for writes, ACK/NACK under software control for reads.
    a.nop_with_delay(3);
    a.out_with_delay(OutDestination::PINDIRS, 1, 7);
    a.nop_with_side_set(0);
    a.wait_with_delay(1, WaitSource::PIN, scl_offset, false, 6);
    a.in_with_delay(InSource::PINS, 1, 7);
    a.nop_with_delay_and_side_set(2, 1);
    a.push(false, true);
    a.bind(&mut entry);
    a.out(OutDestination::X, 6);
    a.out(OutDestination::NULL, 1); // upstream's FINAL bit is reserved here
    a.jmp(JmpCondition::XIsZero, &mut byte);
    a.out(OutDestination::NULL, 32);
    a.bind(&mut exec);
    a.out(OutDestination::EXEC, 16);
    a.jmp(JmpCondition::XDecNonZero, &mut exec);
    a.bind(&mut end);
    a.assemble_with_wrap(end, entry)
}

/// Round upward so SCL never exceeds the requested rate. Bound quantization
/// jitter by requiring at least 32 MHz sysclk, as in the LED waveform driver.
pub(crate) fn divider(sys_hz: u32, frequency: u32) -> Option<u32> {
    if sys_hz < 32_000_000 || frequency == 0 || frequency > 400_000 {
        return None;
    }
    let div = (u64::from(sys_hz) * 256).div_ceil(u64::from(frequency) * CYCLES_PER_BIT);
    (256..=0xff_ffff).contains(&div).then_some(div as u32)
}

/// Replicate the halfword, equivalent to upstream's narrow MMIO stores.
pub(crate) fn fifo_word(word: u16) -> u32 {
    u32::from(word) * 0x0001_0001
}

pub(crate) fn byte(value: u8, nack: bool) -> u16 {
    (u16::from(!value) << 1) | u16::from(!nack)
}

/// Short instruction packets, terminated by a PUSH completion token.
pub(crate) struct Commands {
    words: [u16; 32],
    len: usize,
}

impl Commands {
    pub fn new() -> Self {
        Self {
            words: [0; 32],
            len: 1,
        }
    }

    fn append(&mut self, a: Assembler<32>) {
        for word in a.assemble_program().code {
            self.words[self.len] = word;
            self.len += 1;
        }
    }

    /// A phase lasts at least 20 cycles (plus OUT EXEC overhead), meeting
    /// START/STOP setup/hold and bus-free minima at both 100 and 400 kHz.
    pub fn pins(&mut self, scl: bool, sda: bool) {
        let mut a = Assembler::<32>::new_with_side_set(SIDE);
        a.set_with_delay_and_side_set(SetDestination::PINDIRS, u8::from(!sda), 7, u8::from(!scl));
        a.nop_with_delay(7);
        a.nop_with_delay(3);
        self.append(a);
    }

    pub fn wait_high(&mut self, offset: u8) {
        let mut a = Assembler::<32>::new_with_side_set(SIDE);
        a.wait(1, WaitSource::PIN, offset, false);
        self.append(a);
    }

    pub fn finish(mut self) -> Self {
        let mut a = Assembler::<32>::new_with_side_set(SIDE);
        a.mov(MovDestination::ISR, MovOperation::None, MovSource::NULL);
        a.push(false, true);
        self.append(a);
        self.words[0] = ((self.len - 2) as u16) << 10;
        self
    }

    pub fn words(&self) -> &[u16] {
        &self.words[..self.len]
    }
}

#[cfg(test)]
pub(crate) mod tests;
