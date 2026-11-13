//! Async eight-bit PCA9534/TCA9534 and PCA9534A/TCA9534A GPIO expanders.
//!
//! Uses `embedded-hal-async` 1.0 without allocation, a runtime or an MCU feature.
//! Construction performs no I/O. Registers are never cached: masked updates read
//! the current register before writing, including after a failed/cancelled write.
//! Keep one logical owner per physical expander; a shared bus lock alone does not
//! serialize the two transactions of a read-modify-write against another owner.
//!
//! Errors are the original bus errors, with no automatic retry. Cancellation
//! inherits the bus's contract. A failed/cancelled operation may have changed the
//! hardware; recover the bus and explicitly read back or reapply configuration.
//! Power sequencing, INT monitoring, debounce and deadlines belong to the caller.
//!
//! ```no_run
//! use embedded_hal_async::i2c::I2c;
//! use polis_xca953xa::{Address, Config, Level, Pin, Xca9534};
//!
//! async fn example<B: I2c>(bus: B) -> Result<(), B::Error> {
//!     let mut port = Xca9534::new(bus, Address::DEFAULT);
//!     // P0 output initially low; P1..P7 inputs, with normal polarity.
//!     port.init(Config { outputs: 0, polarity: 0, inputs: 0xfe }).await?;
//!     let sample = port.read_pin(Pin::P1).await?;
//!     port.set_level(Pin::P0, sample).await?;
//!     port.set_level(Pin::P0, Level::Low).await
//! }
//! ```
#![no_std]
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use embedded_hal_async::i2c::I2c;

/// Address variant, common to both PCA and TCA parts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Variant {
    /// PCA9534 / TCA9534: 0x20..=0x27.
    Standard,
    /// PCA9534A / TCA9534A: 0x38..=0x3f.
    A,
}

/// Validated, unshifted seven-bit address for the supported eight-bit parts.
///
/// Select the range matching the fitted device; there is no device-ID register
/// or automatic identification. Other devices can acknowledge the same address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Address(u8);

impl Address {
    /// PCA9534 / TCA9534 with A2, A1 and A0 low.
    pub const DEFAULT: Self = Self(0x20);
    /// PCA9534A / TCA9534A with A2, A1 and A0 low.
    pub const A_DEFAULT: Self = Self(0x38);

    /// Validate 0x20..=0x27 or 0x38..=0x3f; reject all other values.
    pub const fn new(address: u8) -> Option<Self> {
        match address {
            0x20..=0x27 | 0x38..=0x3f => Some(Self(address)),
            _ => None,
        }
    }

    /// Select the part variant and A2/A1/A0 strap levels (`true` means high).
    pub const fn from_pins(variant: Variant, a2: bool, a1: bool, a0: bool) -> Self {
        let base = match variant {
            Variant::Standard => 0x20,
            Variant::A => 0x38,
        };
        Self(base | ((a2 as u8) << 2) | ((a1 as u8) << 1) | a0 as u8)
    }

    /// Return the unshifted seven-bit address.
    pub const fn as_u8(self) -> u8 {
        self.0
    }
}

/// One of the eight port pins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Pin {
    /// Port pin 0.
    P0,
    /// Port pin 1.
    P1,
    /// Port pin 2.
    P2,
    /// Port pin 3.
    P3,
    /// Port pin 4.
    P4,
    /// Port pin 5.
    P5,
    /// Port pin 6.
    P6,
    /// Port pin 7.
    P7,
}

impl Pin {
    /// Validate a zero-based pin number.
    pub const fn new(pin: u8) -> Option<Self> {
        match pin {
            0 => Some(Self::P0),
            1 => Some(Self::P1),
            2 => Some(Self::P2),
            3 => Some(Self::P3),
            4 => Some(Self::P4),
            5 => Some(Self::P5),
            6 => Some(Self::P6),
            7 => Some(Self::P7),
            _ => None,
        }
    }

    /// The corresponding bit in a port register.
    pub const fn mask(self) -> u8 {
        1 << self as u8
    }
}

/// Output-latch level, or a sampled input after configured polarity inversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    /// Register bit zero.
    Low,
    /// Register bit one.
    High,
}

/// Port pin direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    /// High-impedance input (configuration bit one).
    Input,
    /// Push-pull output (configuration bit zero).
    Output,
}

/// Input polarity; does not invert the output latch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Polarity {
    /// Report the input level without inversion.
    Normal,
    /// Invert the reported input level.
    Inverted,
}

/// Explicit whole-port initialization values; one bit per pin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    /// Output-latch levels: one means high, zero means low.
    pub outputs: u8,
    /// Input polarity: one means inverted, zero means normal.
    pub polarity: u8,
    /// Directions: one means high-impedance input, zero means push-pull output.
    pub inputs: u8,
}

impl Default for Config {
    /// Documented power-on register values, not a claim about current hardware.
    fn default() -> Self {
        Self {
            outputs: 0xff,
            polarity: 0,
            inputs: 0xff,
        }
    }
}

#[derive(Clone, Copy)]
enum Register {
    Input = 0,
    Output = 1,
    Polarity = 2,
    Configuration = 3,
}

/// One PCA9534/TCA9534 or PCA9534A/TCA9534A, with no cached device state.
///
/// Own or borrow an async I²C bus, including an Embassy shared-bus proxy.
/// Serialize access to this driver across complete method calls when sharing it.
pub struct Xca9534<BUS> {
    bus: BUS,
    address: Address,
}

impl<BUS> Xca9534<BUS> {
    /// Construct without I/O, probing or changing any pins.
    pub const fn new(bus: BUS, address: Address) -> Self {
        Self { bus, address }
    }

    /// The selected address.
    pub const fn address(&self) -> Address {
        self.address
    }

    /// Return the bus without changing the expander's outputs or configuration.
    pub fn into_inner(self) -> BUS {
        self.bus
    }

    /// Access the bus for caller-managed recovery. There is no cache to invalidate.
    pub fn bus_mut(&mut self) -> &mut BUS {
        &mut self.bus
    }
}

impl<BUS: I2c> Xca9534<BUS> {
    /// Release every pin to input, load latches and polarity, then set directions.
    ///
    /// This is four separate writes, not an atomic operation or a hardware reset.
    /// Existing outputs temporarily become high impedance; use only when the
    /// board permits that transition. Stops at the first bus error. Failure or
    /// cancellation can leave any completed prefix applied, including the final
    /// write. Recover the bus before explicitly reading back or reapplying.
    pub async fn init(&mut self, config: Config) -> Result<(), BUS::Error> {
        self.write_configuration(0xff).await?;
        self.write_outputs(config.outputs).await?;
        self.write_polarity(config.polarity).await?;
        self.write_configuration(config.inputs).await
    }

    /// Sample all pins via the input register, with configured input inversion.
    ///
    /// Reads physical pin levels, not output-latch contents. Reading this register
    /// acknowledges the port interrupt according to the device's INT timing;
    /// even a read that later fails/cancels may have acknowledged it. This is a
    /// level sample, not a lossless edge log. No debounce or INT GPIO is supplied.
    pub async fn read_inputs(&mut self) -> Result<u8, BUS::Error> {
        self.read(Register::Input).await
    }

    /// Read the output latches, including those of pins currently set as inputs.
    /// These are commanded levels, not physical pin feedback.
    pub async fn read_outputs(&mut self) -> Result<u8, BUS::Error> {
        self.read(Register::Output).await
    }

    /// Read input polarity bits (one means inverted).
    pub async fn read_polarity(&mut self) -> Result<u8, BUS::Error> {
        self.read(Register::Polarity).await
    }

    /// Read direction bits (one means input, zero means output).
    pub async fn read_configuration(&mut self) -> Result<u8, BUS::Error> {
        self.read(Register::Configuration).await
    }

    /// Write all output latches without changing directions or input polarity.
    pub async fn write_outputs(&mut self, levels: u8) -> Result<(), BUS::Error> {
        self.write(Register::Output, levels).await
    }

    /// Write all input polarity bits (one means inverted).
    pub async fn write_polarity(&mut self, inverted: u8) -> Result<(), BUS::Error> {
        self.write(Register::Polarity, inverted).await
    }

    /// Write all directions (one means input, zero means output).
    /// Preload output latches before enabling outputs if their values are unknown.
    pub async fn write_configuration(&mut self, inputs: u8) -> Result<(), BUS::Error> {
        self.write(Register::Configuration, inputs).await
    }

    /// Update selected output latches, preserving bits outside `mask` by readback.
    ///
    /// A zero mask performs no I/O. Otherwise reads and then writes even if the
    /// value matches. `levels` bits outside `mask` are ignored. Requires exclusive
    /// device ownership across both transactions; no automatic retry on failure.
    pub async fn update_outputs(&mut self, mask: u8, levels: u8) -> Result<(), BUS::Error> {
        self.update(Register::Output, mask, levels).await
    }

    /// Update selected input polarity bits; same ownership/zero-mask rules as
    /// [`Self::update_outputs`]. Bits outside `mask` are preserved by readback.
    pub async fn update_polarity(&mut self, mask: u8, inverted: u8) -> Result<(), BUS::Error> {
        self.update(Register::Polarity, mask, inverted).await
    }

    /// Update selected directions; same ownership/zero-mask rules as
    /// [`Self::update_outputs`]. Preload latches before enabling outputs.
    pub async fn update_configuration(&mut self, mask: u8, inputs: u8) -> Result<(), BUS::Error> {
        self.update(Register::Configuration, mask, inputs).await
    }

    /// Sample one pin. Reads the entire input register and can acknowledge INT
    /// for other pins too; see [`Self::read_inputs`].
    pub async fn read_pin(&mut self, pin: Pin) -> Result<Level, BUS::Error> {
        Ok(if self.read_inputs().await? & pin.mask() == 0 {
            Level::Low
        } else {
            Level::High
        })
    }

    /// Update one output latch without changing direction or other latches.
    pub async fn set_level(&mut self, pin: Pin, level: Level) -> Result<(), BUS::Error> {
        let value = if level == Level::High { pin.mask() } else { 0 };
        self.update_outputs(pin.mask(), value).await
    }

    /// Update one direction, preserving the others. Preload its output latch
    /// with [`Self::set_level`] before changing an input to an output.
    pub async fn set_direction(
        &mut self,
        pin: Pin,
        direction: Direction,
    ) -> Result<(), BUS::Error> {
        let value = if direction == Direction::Input {
            pin.mask()
        } else {
            0
        };
        self.update_configuration(pin.mask(), value).await
    }

    /// Update one input polarity bit without changing output latches.
    pub async fn set_polarity(&mut self, pin: Pin, polarity: Polarity) -> Result<(), BUS::Error> {
        let value = if polarity == Polarity::Inverted {
            pin.mask()
        } else {
            0
        };
        self.update_polarity(pin.mask(), value).await
    }

    async fn read(&mut self, register: Register) -> Result<u8, BUS::Error> {
        let mut data = [0];
        self.bus
            .write_read(self.address.0, &[register as u8], &mut data)
            .await?;
        Ok(data[0])
    }

    async fn write(&mut self, register: Register, value: u8) -> Result<(), BUS::Error> {
        self.bus
            .write(self.address.0, &[register as u8, value])
            .await
    }

    async fn update(&mut self, register: Register, mask: u8, value: u8) -> Result<(), BUS::Error> {
        if mask == 0 {
            return Ok(());
        }
        let previous = self.read(register).await?;
        self.write(register, (previous & !mask) | (value & mask))
            .await
    }
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
