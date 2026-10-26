//! Async PCA9548A/TCA9548A control and channel buses, with no allocation or selected MCU.
//!
//! [`Xca9548a`] owns the upstream bus. Its borrowed [`ChannelBus`] implements
//! [`embedded_hal_async::i2c::I2c`]. Enable `embassy` for repeatable shared channel
//! handles from `SharedMux`. Construction performs no I/O.
//! `Xca9548a` supports the eight-channel PCA9548A and TCA9548A; other XCA954xA
//! variants are not yet supported by this package.
//!
//! One manager must exclusively control the mux and upstream bus. Cascaded muxes,
//! other bus masters, and uncoordinated upstream access are outside this contract.
//! The caller owns RESET, power sequencing, timeouts and bus recovery. Invalidate
//! the selection cache after any external reset or power loss, before reuse.
#![no_std]
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use embedded_hal::i2c::{ErrorKind, ErrorType, Operation};
use embedded_hal_async::i2c::I2c;

/// Unshifted seven-bit mux address, in 0x70..=0x77.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Address(u8);

impl Address {
    /// All three address pins low: 0x70.
    pub const DEFAULT: Self = Self(0x70);

    /// Validate an unshifted address. Returns `None` outside 0x70..=0x77.
    pub const fn new(address: u8) -> Option<Self> {
        if address >= 0x70 && address <= 0x77 {
            Some(Self(address))
        } else {
            None
        }
    }

    /// Address selected by the A2, A1 and A0 pins, respectively.
    pub const fn from_pins(a2: bool, a1: bool, a0: bool) -> Self {
        Self(0x70 | ((a2 as u8) << 2) | ((a1 as u8) << 1) | a0 as u8)
    }

    /// Return the unshifted seven-bit address.
    pub const fn as_u8(self) -> u8 {
        self.0
    }
}

impl Default for Address {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A single downstream SDn/SCn pair.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Channel {
    /// SD0/SC0.
    Ch0,
    /// SD1/SC1.
    Ch1,
    /// SD2/SC2.
    Ch2,
    /// SD3/SC3.
    Ch3,
    /// SD4/SC4.
    Ch4,
    /// SD5/SC5.
    Ch5,
    /// SD6/SC6.
    Ch6,
    /// SD7/SC7.
    Ch7,
}

impl Channel {
    /// Validate a zero-based channel number.
    pub const fn new(channel: u8) -> Option<Self> {
        match channel {
            0 => Some(Self::Ch0),
            1 => Some(Self::Ch1),
            2 => Some(Self::Ch2),
            3 => Some(Self::Ch3),
            4 => Some(Self::Ch4),
            5 => Some(Self::Ch5),
            6 => Some(Self::Ch6),
            7 => Some(Self::Ch7),
            _ => None,
        }
    }

    /// Control byte that enables only this channel.
    pub const fn mask(self) -> u8 {
        1 << self as u8
    }
}

/// Failure selecting a route or transferring to the downstream device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<E> {
    /// Failure accessing the mux control register; preserves the bus error.
    Select(E),
    /// Failure in the downstream transaction; preserves the bus error.
    Transfer(E),
    /// Reserved/non-seven-bit address or the mux's own address.
    InvalidDeviceAddress(u8),
}

impl<E: embedded_hal::i2c::Error> embedded_hal::i2c::Error for Error<E> {
    fn kind(&self) -> ErrorKind {
        match self {
            Self::Select(error) | Self::Transfer(error) => error.kind(),
            Self::InvalidDeviceAddress(_) => ErrorKind::Other,
        }
    }
}

/// Portable PCA9548A/TCA9548A driver with a cached control byte, initially unknown.
pub struct Xca9548a<BUS> {
    bus: BUS,
    address: Address,
    selected: Option<u8>,
}

impl<BUS> Xca9548a<BUS> {
    /// Take ownership of a bus without probing, selecting or resetting hardware.
    pub const fn new(bus: BUS, address: Address) -> Self {
        Self {
            bus,
            address,
            selected: None,
        }
    }

    /// Return the upstream bus without changing the hardware selection.
    pub fn into_inner(self) -> BUS {
        self.bus
    }

    /// Forget the cached selection after RESET, power loss or external changes.
    ///
    /// Performs no I/O. The next channel operation will explicitly select again.
    pub fn invalidate_selection(&mut self) {
        self.selected = None;
    }

    /// Access the upstream bus for recovery, invalidating cached selection first.
    ///
    /// The caller must restore bus readiness before using a channel again.
    pub fn bus_mut(&mut self) -> &mut BUS {
        self.invalidate_selection();
        &mut self.bus
    }

    /// Borrow a channel bus exclusively, without a mutex or runtime dependency.
    ///
    /// For multiple simultaneous handles, enable `embassy` and use `SharedMux`.
    pub fn channel(&mut self, channel: Channel) -> ChannelBus<'_, BUS> {
        ChannelBus { mux: self, channel }
    }
}

impl<BUS: I2c> Xca9548a<BUS> {
    /// Enable only the requested channel; skip the write if already known active.
    pub async fn select(&mut self, channel: Channel) -> Result<(), Error<BUS::Error>> {
        self.select_channels(channel.mask()).await
    }

    /// Set the complete control mask; zero disconnects every downstream channel.
    ///
    /// Multi-channel masks are for explicit low-level use. The caller must ensure
    /// simultaneously connected devices have no conflicting addresses. Channel
    /// handles always replace this mask with exactly one selected channel.
    pub async fn select_channels(&mut self, mask: u8) -> Result<(), Error<BUS::Error>> {
        if self.selected != Some(mask) {
            // Invalidate before awaiting: even a failed/cancelled write may have
            // reached the hardware. Only a completed write confirms selection.
            self.selected = None;
            self.bus
                .write(self.address.0, &[mask])
                .await
                .map_err(Error::Select)?;
            // The standalone write supplies the STOP needed to activate channels.
            self.selected = Some(mask);
        }
        Ok(())
    }

    /// Disconnect every downstream channel.
    pub async fn disable_all(&mut self) -> Result<(), Error<BUS::Error>> {
        self.select_channels(0).await
    }

    /// Read the hardware control byte and refresh the cache.
    pub async fn read_channels(&mut self) -> Result<u8, Error<BUS::Error>> {
        self.selected = None;
        let mut bytes = [0];
        self.bus
            .read(self.address.0, &mut bytes)
            .await
            .map_err(Error::Select)?;
        self.selected = Some(bytes[0]);
        Ok(bytes[0])
    }

    async fn channel_transaction(
        &mut self,
        channel: Channel,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Error<BUS::Error>> {
        // Prevent bypassing route management and reserved-address commands.
        if !(0x08..=0x77).contains(&address) || address == self.address.0 {
            return Err(Error::InvalidDeviceAddress(address));
        }
        if operations.is_empty() {
            return Ok(());
        }
        self.select(channel).await?;
        // Conservatively forget the selection on downstream error/cancellation
        // as well, so recovery cannot silently reuse an uncertain route.
        self.selected = None;
        self.bus
            .transaction(address, operations)
            .await
            .map_err(Error::Transfer)?;
        self.selected = Some(channel.mask());
        Ok(())
    }
}

/// Exclusively borrowed virtual bus implementing async seven-bit I²C.
///
/// Each transaction selects exactly one channel, followed by the caller's
/// unchanged operations. Bus errors/cancellation require caller-managed recovery.
pub struct ChannelBus<'a, BUS> {
    mux: &'a mut Xca9548a<BUS>,
    channel: Channel,
}

impl<BUS: ErrorType> ErrorType for ChannelBus<'_, BUS> {
    type Error = Error<BUS::Error>;
}

impl<BUS: I2c> I2c for ChannelBus<'_, BUS> {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.mux
            .channel_transaction(self.channel, address, operations)
            .await
    }
}

#[cfg(feature = "embassy")]
mod shared;
#[cfg(feature = "embassy")]
pub use shared::{SharedChannel, SharedMux};

#[cfg(test)]
mod tests;
