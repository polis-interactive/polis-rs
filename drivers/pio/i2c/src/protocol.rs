// START/STOP/escape encoding adapted from pico-examples/pio/i2c/pio_i2c.c.
// Copyright (c) 2021 Raspberry Pi (Trading) Ltd.
// SPDX-License-Identifier: BSD-3-Clause

use crate::{
    Error, NoAcknowledgeSource, Operation,
    waveform::{self, Commands},
};

pub(crate) trait Bus {
    /// Execute a single byte record or an instruction packet; return its RX word.
    async fn exchange(&mut self, words: &[u16]) -> Result<u32, Error>;
    fn scl_offset(&self) -> u8;
}

pub(crate) fn validate(address: u8, ops: &[Operation<'_>]) -> Result<(), Error> {
    if address > 0x7f {
        return Err(Error::AddressOutOfRange(address));
    }
    if ops
        .iter()
        .any(|op| matches!(op, Operation::Read(buf) if buf.is_empty()))
    {
        return Err(Error::InvalidReadBufferLength);
    }
    Ok(())
}

async fn start(bus: &mut impl Bus, repeated: bool) -> Result<(), Error> {
    let mut c = Commands::new();
    if repeated {
        c.pins(false, true);
    }
    c.pins(true, true);
    c.wait_high(bus.scl_offset());
    c.wait_high(0);
    // Delay after SCL actually rises (including clock stretching).
    c.pins(true, true);
    c.pins(true, false);
    c.pins(false, false);
    bus.exchange(c.finish().words()).await?;
    Ok(())
}

async fn stop(bus: &mut impl Bus) -> Result<(), Error> {
    let mut c = Commands::new();
    c.pins(false, false);
    c.pins(true, false);
    c.wait_high(bus.scl_offset());
    c.pins(true, false);
    c.pins(true, true);
    c.wait_high(0);
    // Bus-free interval starts only after both physical lines are high.
    c.pins(true, true);
    bus.exchange(c.finish().words()).await?;
    Ok(())
}

async fn write_byte(
    bus: &mut impl Bus,
    value: u8,
    source: NoAcknowledgeSource,
) -> Result<(), Error> {
    let sample = bus.exchange(&[waveform::byte(value, true)]).await?;
    if sample & 1 != 0 {
        return Err(Error::NoAcknowledge(source));
    }
    Ok(())
}

pub(crate) async fn transaction(
    bus: &mut impl Bus,
    address: u8,
    ops: &mut [Operation<'_>],
) -> Result<(), Error> {
    if ops.is_empty() {
        return Ok(());
    }
    let result = transfer(bus, address, ops).await;
    // NACK still ends with STOP. A timeout cannot safely continue the exchange.
    if result.is_ok() || matches!(result, Err(Error::NoAcknowledge(_))) {
        stop(bus).await?;
    }
    result
}

async fn transfer(bus: &mut impl Bus, address: u8, ops: &mut [Operation<'_>]) -> Result<(), Error> {
    let mut previous_read = None;
    for index in 0..ops.len() {
        let read = matches!(ops[index], Operation::Read(_));
        let last_read = !matches!(ops.get(index + 1), Some(Operation::Read(_)));
        if previous_read != Some(read) {
            start(bus, previous_read.is_some()).await?;
            write_byte(
                bus,
                (address << 1) | u8::from(read),
                NoAcknowledgeSource::Address,
            )
            .await?;
        }
        match &mut ops[index] {
            Operation::Write(bytes) => {
                for &byte in *bytes {
                    write_byte(bus, byte, NoAcknowledgeSource::Data).await?;
                }
            }
            Operation::Read(bytes) => {
                let len = bytes.len();
                for (i, byte) in bytes.iter_mut().enumerate() {
                    let nack = last_read && i + 1 == len;
                    *byte = (bus.exchange(&[waveform::byte(0xff, nack)]).await? >> 1) as u8;
                }
            }
        }
        previous_read = Some(read);
    }
    Ok(())
}

/// Single-controller bus clear: nine SCL pulses with SDA released, then STOP.
pub(crate) async fn recover(bus: &mut impl Bus) -> Result<(), Error> {
    for _ in 0..9 {
        let mut c = Commands::new();
        c.pins(false, true);
        c.pins(true, true);
        c.wait_high(bus.scl_offset());
        c.pins(true, true);
        bus.exchange(c.finish().words()).await?;
    }
    stop(bus).await
}

#[cfg(test)]
mod tests;
