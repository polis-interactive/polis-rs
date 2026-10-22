#![no_std]
#![deny(missing_docs)]
//! Async I²C driver for Arduino Modulino Buttons (ABX00110), using stock firmware.
//!
//! Reads all three buttons and controls all three indicator LEDs without an
//! allocator, MCU dependency or executor. The bus may be owned or borrowed.
//! Construction performs no I/O. Polling, debounce, edge detection, timeouts,
//! startup delays and bus recovery belong to the caller.
//!
//! The default seven-bit address is **0x3e**, not Arduino's shifted **0x7c**.
//! Each read is four bytes: the pinstrap identifier (0x7c), then A, B and C.
//! Each LED write is three bytes, one boolean per LED, without a register prefix.
//! Protocol references and pinned revisions are recorded in `PROVENANCE.md`.
//!
//! ```no_run
//! use embedded_hal_async::i2c::I2c;
//! use polis_modulino_buttons::{Error, ModulinoButtons};
//!
//! async fn mirror_buttons<B: I2c>(bus: B) -> Result<(), Error<B::Error>> {
//!     let mut buttons = ModulinoButtons::new(bus);
//!     let state = buttons.read_buttons().await?;
//!     buttons.set_leds(state.a, state.b, state.c).await
//! }
//! ```

use embedded_hal_async::i2c::I2c;

/// Default unshifted seven-bit I²C address of Modulino Buttons.
pub const DEFAULT_ADDRESS: u8 = 0x3e;
const PINSTRAP_ID: u8 = 0x7c;

/// One of the three buttons, labeled on the board.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Button {
    /// Button A.
    A,
    /// Button B.
    B,
    /// Button C.
    C,
}

/// Button levels from one successful read; `true` means pressed.
///
/// These are instantaneous levels, not debounced events. A short press between
/// reads may be missed. The default value means all released, not a sampled state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ButtonState {
    /// Whether A is pressed.
    pub a: bool,
    /// Whether B is pressed.
    pub b: bool,
    /// Whether C is pressed.
    pub c: bool,
}

impl ButtonState {
    /// Test the level of one labeled button without another bus transaction.
    pub const fn is_pressed(self, button: Button) -> bool {
        match button {
            Button::A => self.a,
            Button::B => self.b,
            Button::C => self.c,
        }
    }
}

/// An address outside the non-reserved seven-bit range 0x08..=0x77.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidAddress(pub u8);

/// A transport failure or a response from an unexpected module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<E> {
    /// The underlying bus error, preserved without automatic retries.
    I2c(E),
    /// The first read byte was not the Buttons pinstrap identifier (0x7c).
    UnexpectedDevice {
        /// The identifier returned by the device.
        pinstrap_id: u8,
    },
}

/// An Arduino Modulino Buttons module on an asynchronous I²C bus.
///
/// No button or LED state is cached. A failed read returns no sample, and a
/// failed or cancelled LED write may have affected the device. Recover the bus
/// according to its HAL's requirements before explicitly reapplying LED state.
pub struct ModulinoButtons<BUS> {
    bus: BUS,
    address: u8,
}

impl<BUS> ModulinoButtons<BUS> {
    /// Use the factory seven-bit address (0x3e), without communicating.
    pub const fn new(bus: BUS) -> Self {
        Self {
            bus,
            address: DEFAULT_ADDRESS,
        }
    }

    /// Use an already configured, unshifted seven-bit address.
    ///
    /// Rejects reserved addresses and values above seven bits, including 0x7c.
    /// This only selects the host address; it does not reprogram the module or
    /// perform I/O. Pass a borrowed bus if it must remain available on error.
    pub fn with_address(bus: BUS, address: u8) -> Result<Self, InvalidAddress> {
        if !(0x08..=0x77).contains(&address) {
            return Err(InvalidAddress(address));
        }
        Ok(Self { bus, address })
    }

    /// The selected unshifted seven-bit address.
    pub const fn address(&self) -> u8 {
        self.address
    }

    /// Return the owned bus or original bus borrow.
    pub fn into_inner(self) -> BUS {
        self.bus
    }

    /// Access the bus for caller-managed recovery.
    pub fn bus_mut(&mut self) -> &mut BUS {
        &mut self.bus
    }
}

impl<BUS: I2c> ModulinoButtons<BUS> {
    /// Read A/B/C in one four-byte transaction with no register-pointer write.
    ///
    /// Validates the hardware pinstrap identifier, which remains 0x7c even at a
    /// custom bus address. Zero means released; any nonzero button byte means
    /// pressed, matching the Arduino library. No debounce or edge tracking occurs.
    pub async fn read_buttons(&mut self) -> Result<ButtonState, Error<BUS::Error>> {
        let mut bytes = [0; 4];
        self.bus
            .read(self.address, &mut bytes)
            .await
            .map_err(Error::I2c)?;
        if bytes[0] != PINSTRAP_ID {
            return Err(Error::UnexpectedDevice {
                pinstrap_id: bytes[0],
            });
        }
        Ok(ButtonState {
            a: bytes[1] != 0,
            b: bytes[2] != 0,
            c: bytes[3] != 0,
        })
    }

    /// Set LEDs A/B/C in one three-byte write; `true` means on.
    ///
    /// All three states must be supplied on every call. This does not read or
    /// validate the module identifier; call `read_buttons` first to check it.
    /// Success acknowledges the transfer, not optical feedback or LED readback.
    /// Firmware applies the write asynchronously; caller polling should leave
    /// time between writes for it to process them.
    pub async fn set_leds(&mut self, a: bool, b: bool, c: bool) -> Result<(), Error<BUS::Error>> {
        self.bus
            .write(self.address, &[u8::from(a), u8::from(b), u8::from(c)])
            .await
            .map_err(Error::I2c)
    }
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
