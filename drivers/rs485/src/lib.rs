#![no_std]
//! Async half-duplex RS-485 using embedded-io-async, a direction GPIO and delay.
//!
//! No HAL, PIO, executor, packet framing, addressing or bus arbitration dependency.
//! The UART's Write::flush MUST complete the final stop bit. A no-op or merely
//! FIFO-empty flush is unsuitable. See README for cancellation and recovery.

use core::convert::Infallible;
use embedded_hal::digital::{OutputPin, PinState};
use embedded_hal_async::delay::DelayNs;
use embedded_io_async::{ErrorType, Read, Write};

/// Electrical direction and minimum settling times, chosen by board support.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Direction output level enabling transmission (opposite enables receive).
    pub tx_level: PinState,
    /// Delay after asserting transmit, before queuing the first byte.
    pub enable_us: u32,
    /// Extra hold after flush has completed the final stop bit.
    pub hold_us: u32,
    /// Receiver settling delay after releasing transmit.
    pub receive_us: u32,
}

/// Driver or underlying stream error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error<E> {
    Io(E),
    /// Call recover after construction, failed send or cancelled send/recovery.
    RecoveryRequired,
    /// The underlying writer made no progress on a nonempty buffer.
    WriteZero,
}

impl<E: core::fmt::Display> core::fmt::Display for Error<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "serial error: {e}"),
            Self::RecoveryRequired => f.write_str("RS-485 recovery required"),
            Self::WriteZero => f.write_str("serial write made no progress"),
        }
    }
}

impl<E: core::error::Error> core::error::Error for Error<E> {}

impl<E: embedded_io_async::Error> embedded_io_async::Error for Error<E> {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        match self {
            Self::Io(e) => e.kind(),
            Self::RecoveryRequired => embedded_io_async::ErrorKind::Other,
            Self::WriteZero => embedded_io_async::ErrorKind::WriteZero,
        }
    }
}

/// Owned serial stream and tied DE/RE direction output.
///
/// Local GPIO must be infallible so a future's destructor can release the bus.
/// Reads and sends are exclusive; the underlying UART owns receive buffering.
pub struct Rs485<S, D: OutputPin<Error = Infallible>, T> {
    serial: S,
    direction: D,
    delay: T,
    config: Config,
    recovery_required: bool,
}

fn set<D: OutputPin<Error = Infallible>>(pin: &mut D, state: PinState) {
    let Ok(()) = pin.set_state(state);
}

impl<S, D: OutputPin<Error = Infallible>, T> Rs485<S, D, T> {
    /// Immediately select receive. Call recover before the first operation to
    /// flush any pre-existing TX data while the transceiver is disabled.
    /// BSP must initially configure direction at the receive level too.
    pub fn new(serial: S, mut direction: D, delay: T, config: Config) -> Self {
        set(&mut direction, !config.tx_level);
        Self {
            serial,
            direction,
            delay,
            config,
            recovery_required: true,
        }
    }

    pub fn recovery_required(&self) -> bool {
        self.recovery_required
    }
}

impl<S: Write, D: OutputPin<Error = Infallible>, T: DelayNs> Rs485<S, D, T> {
    /// Drain pending TX with the transceiver DISABLED, then wait for receive
    /// settling. Pending bytes are intentionally not delivered to the RS-485
    /// bus. Does not purge RX or repair packet state. Cancellation/error leaves
    /// recovery required. Apply a caller timeout if flush can stall.
    pub async fn recover(&mut self) -> Result<(), Error<S::Error>> {
        self.recovery_required = true;
        set(&mut self.direction, !self.config.tx_level);
        self.serial.flush().await.map_err(Error::Io)?;
        self.delay.delay_us(self.config.receive_us).await;
        self.recovery_required = false;
        Ok(())
    }

    /// Send the complete slice during one direction assertion, including short
    /// underlying writes. Return after final stop bit, hold and receive delays.
    ///
    /// Caller must own the bus slot. On error/cancellation DE is released
    /// synchronously; bytes can still drain inside the UART. recover MUST finish
    /// before sending again. The peer may have received a partial message.
    /// Empty sends do nothing, including while recovery is required.
    pub async fn send(&mut self, mut bytes: &[u8]) -> Result<(), Error<S::Error>> {
        if bytes.is_empty() {
            return Ok(());
        }
        if self.recovery_required {
            return Err(Error::RecoveryRequired);
        }
        self.recovery_required = true;
        let guard = DirectionGuard {
            pin: &mut self.direction,
            receive: !self.config.tx_level,
        };
        set(guard.pin, self.config.tx_level);
        self.delay.delay_us(self.config.enable_us).await;
        while !bytes.is_empty() {
            let count = self.serial.write(bytes).await.map_err(Error::Io)?;
            if count == 0 {
                return Err(Error::WriteZero);
            }
            bytes = &bytes[count..];
        }
        self.serial.flush().await.map_err(Error::Io)?;
        self.delay.delay_us(self.config.hold_us).await;
        drop(guard);
        self.delay.delay_us(self.config.receive_us).await;
        self.recovery_required = false;
        Ok(())
    }
}

impl<S: ErrorType, D: OutputPin<Error = Infallible>, T> ErrorType for Rs485<S, D, T> {
    type Error = Error<S::Error>;
}

impl<S: Read, D: OutputPin<Error = Infallible>, T> Read for Rs485<S, D, T> {
    /// Cancel safety and partial-buffer/error semantics are those of the serial
    /// reader. No implicit RX purge or packet resynchronization is performed.
    async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.recovery_required {
            return Err(Error::RecoveryRequired);
        }
        self.serial.read(buffer).await.map_err(Error::Io)
    }
}

impl<S: Write, D: OutputPin<Error = Infallible>, T: DelayNs> Write for Rs485<S, D, T> {
    /// A successful write sends all bytes and returns in receive mode.
    async fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
        self.send(bytes).await?;
        Ok(bytes.len())
    }
    /// Successful sends are already flushed. Use recover explicitly after a
    /// failed/cancelled send: this operation never hides a discarded suffix.
    async fn flush(&mut self) -> Result<(), Self::Error> {
        if self.recovery_required {
            Err(Error::RecoveryRequired)
        } else {
            Ok(())
        }
    }
}

impl<S, D: OutputPin<Error = Infallible>, T> Drop for Rs485<S, D, T> {
    fn drop(&mut self) {
        set(&mut self.direction, !self.config.tx_level);
    }
}

struct DirectionGuard<'a, D: OutputPin<Error = Infallible>> {
    pin: &'a mut D,
    receive: PinState,
}

impl<D: OutputPin<Error = Infallible>> Drop for DirectionGuard<'_, D> {
    fn drop(&mut self) {
        set(self.pin, self.receive);
    }
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
