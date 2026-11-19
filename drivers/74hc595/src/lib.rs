#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

use embedded_hal::digital::OutputPin;
use embedded_hal_async::{delay::DelayNs, spi::SpiBus};

/// Configuration or I/O failure. Underlying HAL errors are preserved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<SpiError, LatchError> {
    /// A chain must contain at least one chip.
    EmptyChain,
    /// Shifting or waiting for the physical transfer to finish failed.
    Spi(SpiError),
    /// Setting the storage-clock pin failed; the output state may be uncertain.
    Latch(LatchError),
}

/// A chain of `N` 74HC595 or 74HCT595 chips on an exclusive SPI bus.
///
/// Configure MSB-first mode 0. Connect MOSI to DS, SCK to SHCP, and `latch`
/// to STCP. Supply a delay that honors minimum durations. OE and MR belong
/// to the caller: keep MR high during writes and OE high until initialization
/// succeeds. The driver performs no readback and keeps no output cache.
pub struct Hc595<SPI, LATCH, DELAY, const N: usize = 1> {
    spi: SPI,
    latch: LATCH,
    delay: DELAY,
}

impl<SPI, LATCH, DELAY, const N: usize> Hc595<SPI, LATCH, DELAY, N>
where
    SPI: SpiBus<u8>,
    LATCH: OutputPin,
    DELAY: DelayNs,
{
    /// Construct without I/O. `N` must match the physical chain length.
    ///
    /// Initialize STCP low before construction. Call [`Self::write_outputs`]
    /// or [`Self::clear`] before enabling outputs; power-on contents are unknown.
    /// Resources can be owned or borrowed through `&mut` HAL implementations.
    pub fn new(
        spi: SPI,
        latch: LATCH,
        delay: DELAY,
    ) -> Result<Self, Error<SPI::Error, LATCH::Error>> {
        if N == 0 {
            return Err(Error::EmptyChain);
        }
        Ok(Self { spi, latch, delay })
    }

    /// Return the bus, latch pin and delay without changing hardware.
    pub fn into_inner(self) -> (SPI, LATCH, DELAY) {
        (self.spi, self.latch, self.delay)
    }

    /// Borrow the bus for HAL-specific recovery after an error or cancellation.
    ///
    /// Raw transfers alter the shift registers. Follow recovery with a complete
    /// [`Self::write_outputs`] before relying on the output state.
    pub fn bus_mut(&mut self) -> &mut SPI {
        &mut self.spi
    }

    /// Shift a complete frame and update all storage registers on one latch edge.
    ///
    /// Bytes are in wire order: `outputs[0]` reaches the farthest chip and
    /// `outputs[N - 1]` the chip connected to MOSI. Bit 0 drives Q0, bit 7 Q7.
    /// All bytes are sent even when equal to an earlier frame. The array length
    /// prevents short writes from accidentally retaining old bits in the chain.
    ///
    /// The sequence first waits 1 µs (finishing any cancelled high pulse),
    /// establishes STCP low for at least 1 µs, writes and flushes
    /// SPI, waits at least 1 µs from the final clock, pulses STCP high for at
    /// least 1 µs, then leaves it low. These margins cover Nexperia Rev. 12's
    /// latch pulse/setup minima; SPI rate and electrical limits remain caller-owned.
    ///
    /// # Errors and cancellation
    ///
    /// A write/flush error stops before the latch edge. A latch error may occur
    /// after outputs change. Dropping the future during shifting leaves partial
    /// shift contents; dropping it during the high-pulse delay leaves STCP high
    /// with the new frame already latched. No async cleanup or retries run.
    /// Disable OE if needed, recover the bus, then write a complete frame.
    /// The next write reestablishes STCP low before shifting.
    pub async fn write_outputs(
        &mut self,
        outputs: &[u8; N],
    ) -> Result<(), Error<SPI::Error, LATCH::Error>> {
        self.delay.delay_us(1).await;
        self.latch.set_low().map_err(Error::Latch)?;
        self.delay.delay_us(1).await;
        self.spi.write(outputs).await.map_err(Error::Spi)?;
        self.spi.flush().await.map_err(Error::Spi)?;
        self.delay.delay_us(1).await;
        self.latch.set_high().map_err(Error::Latch)?;
        self.delay.delay_us(1).await;
        self.latch.set_low().map_err(Error::Latch)
    }

    /// Write and latch zeros across the entire chain.
    ///
    /// This clears the storage registers as well as the shift registers; it
    /// does not pulse MR or change OE. Error/cancellation semantics match
    /// [`Self::write_outputs`].
    pub async fn clear(&mut self) -> Result<(), Error<SPI::Error, LATCH::Error>> {
        self.write_outputs(&[0; N]).await
    }
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
