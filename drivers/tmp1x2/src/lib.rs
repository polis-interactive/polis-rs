#![no_std]
#![deny(missing_docs)]
//! Async TMP102/TMP112 temperature sensor driver, independent of MCU and executor.
//!
//! Register definitions follow TI TMP102 datasheet SBOS397I, sections 6.4–6.5,
//! and TMP112 datasheet SBOS473L, sections 7.4–7.5. The implemented measurement
//! registers and sample formats are identical; no chip-selection flag is needed.
//! The caller owns polling, conversion deadlines, bus recovery and population
//! policy. Constructing a driver does not communicate with or reset the sensor.

use embedded_hal_async::i2c::I2c;

const TEMPERATURE: u8 = 0x00;
const CONFIGURATION: u8 = 0x01;
const OS: u16 = 0x8000;
const SD: u16 = 0x0100;
const EM: u16 = 0x0010;
// F1/F0, POL, TM, SD, CR1/CR0 and EM. Excludes OS, read-only and reserved bits.
const WRITABLE: u16 = 0x1fd0;
const MEASUREMENT: u16 = SD | 0x00c0 | EM;

/// Address selected by the TMP102/TMP112 ADD0 pin, as an unshifted seven-bit address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Address {
    /// ADD0 connected to ground: 0x48.
    Ground = 0x48,
    /// ADD0 connected to V+: 0x49.
    Supply = 0x49,
    /// ADD0 connected to SDA: 0x4a.
    Sda = 0x4a,
    /// ADD0 connected to SCL: 0x4b.
    Scl = 0x4b,
}

/// Continuous conversion rate; does not determine application polling frequency.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum ConversionRate {
    /// One conversion every four seconds.
    QuarterHz = 0,
    /// One conversion per second.
    Hz1 = 1,
    /// Four conversions per second (power-up default).
    #[default]
    Hz4 = 2,
    /// Eight conversions per second.
    Hz8 = 3,
}

/// Measurement settings. Defaults match the sensor's power-up measurement mode.
/// Alert polarity, fault queue and thermostat mode are preserved by `configure`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Config {
    /// Continuous conversion frequency.
    pub conversion_rate: ConversionRate,
    /// Stop continuous conversions after the current conversion completes.
    pub shutdown: bool,
    /// Enable the 13-bit temperature format instead of the normal 12-bit format.
    pub extended_mode: bool,
}

impl Config {
    fn from_register(raw: u16) -> Self {
        Self {
            conversion_rate: match (raw >> 6) & 3 {
                0 => ConversionRate::QuarterHz,
                1 => ConversionRate::Hz1,
                2 => ConversionRate::Hz4,
                _ => ConversionRate::Hz8,
            },
            shutdown: raw & SD != 0,
            extended_mode: raw & EM != 0,
        }
    }

    fn bits(self) -> u16 {
        ((self.conversion_rate as u16) << 6)
            | if self.shutdown { SD } else { 0 }
            | if self.extended_mode { EM } else { 0 }
    }
}

/// Signed temperature with the sensor's exact 1/16 °C resolution.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Temperature(i16);

impl Temperature {
    fn from_register(raw: u16) -> Self {
        // D0 identifies the format of the returned sample, avoiding a cached EM
        // setting that could become stale after reset or a failed config write.
        let shift = if raw & 1 == 0 { 4 } else { 3 };
        Self((raw as i16) >> shift)
    }

    /// Exact signed count of 1/16 °C units.
    pub const fn sixteenths_celsius(self) -> i16 {
        self.0
    }

    /// Temperature in milli-Celsius, truncating fractional milli-Celsius toward zero.
    pub const fn milli_celsius(self) -> i32 {
        self.0 as i32 * 125 / 2
    }

    /// Temperature in micro-Celsius, preserving the sensor's exact resolution.
    pub const fn micro_celsius(self) -> i32 {
        self.0 as i32 * 62_500
    }
}

/// Transport failure or an invalid one-shot operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<E> {
    /// Original error from the underlying I²C implementation.
    I2c(E),
    /// One-shot operations require shutdown mode; call `configure` first.
    NotShutdown,
    /// A conversion is still running. Wait for `one_shot_ready` before triggering.
    Busy,
}

/// TMP102 or TMP112 on an owned bus, a mutable reference, or an async bus adapter.
///
/// Use one logical driver per sensor. Configuration is read from the device, not
/// cached. Do not concurrently configure the same sensor through another handle:
/// `configure` is a read-modify-write across two I²C transactions.
pub struct Tmp1x2<BUS> {
    bus: BUS,
    address: Address,
}

impl<BUS> Tmp1x2<BUS> {
    /// Create a handle without probing or changing the sensor.
    pub const fn new(bus: BUS, address: Address) -> Self {
        Self { bus, address }
    }

    /// Return the bus or shared-bus handle.
    pub fn into_inner(self) -> BUS {
        self.bus
    }

    /// Access the bus for caller-managed recovery.
    pub fn bus_mut(&mut self) -> &mut BUS {
        &mut self.bus
    }
}

impl<BUS: I2c> Tmp1x2<BUS> {
    /// Read the most recent conversion using a repeated START.
    ///
    /// This does not trigger or wait for a fresh conversion. Allow the initial
    /// conversion to complete after power-up. Both normal and extended samples
    /// are decoded using the sample's format bit, including negative values.
    pub async fn read_temperature(&mut self) -> Result<Temperature, Error<BUS::Error>> {
        Ok(Temperature::from_register(
            self.read_register(TEMPERATURE).await?,
        ))
    }

    /// Read the sensor's current measurement settings.
    pub async fn read_config(&mut self) -> Result<Config, Error<BUS::Error>> {
        Ok(Config::from_register(
            self.read_register(CONFIGURATION).await?,
        ))
    }

    /// Set measurement settings while preserving thermostat/alert controls.
    ///
    /// Clears the write-only one-shot trigger rather than echoing its read status.
    /// Does not wait for a conversion or rewrite temperature thresholds when EM
    /// changes. A failed/cancelled write may partially change the device;
    /// recover the bus as needed and reapply configuration.
    pub async fn configure(&mut self, config: Config) -> Result<(), Error<BUS::Error>> {
        let raw = self.read_register(CONFIGURATION).await?;
        self.write_config((raw & WRITABLE & !MEASUREMENT) | config.bits())
            .await
    }

    /// Trigger one conversion while in shutdown mode, preserving configuration.
    ///
    /// Returns `Busy` if a previous conversion has not finished. After success,
    /// await readiness using `one_shot_ready` before reading the new temperature.
    /// Cancellation after the write begins may still trigger a conversion.
    pub async fn start_one_shot(&mut self) -> Result<(), Error<BUS::Error>> {
        let raw = self.read_register(CONFIGURATION).await?;
        if raw & SD == 0 {
            return Err(Error::NotShutdown);
        }
        if raw & OS == 0 {
            return Err(Error::Busy);
        }
        self.write_config((raw & WRITABLE) | OS).await
    }

    /// Whether shutdown mode is idle (OS=1), including after a one-shot completes.
    ///
    /// A true result alone does not prove a measurement was triggered. Pair this
    /// with `start_one_shot`. The caller must delay between polls and impose a
    /// deadline; this method performs one I²C transaction and never loops.
    pub async fn one_shot_ready(&mut self) -> Result<bool, Error<BUS::Error>> {
        let raw = self.read_register(CONFIGURATION).await?;
        if raw & SD == 0 {
            return Err(Error::NotShutdown);
        }
        Ok(raw & OS != 0)
    }

    async fn read_register(&mut self, register: u8) -> Result<u16, Error<BUS::Error>> {
        let mut bytes = [0; 2];
        self.bus
            .write_read(self.address as u8, &[register], &mut bytes)
            .await
            .map_err(Error::I2c)?;
        Ok(u16::from_be_bytes(bytes))
    }

    async fn write_config(&mut self, value: u16) -> Result<(), Error<BUS::Error>> {
        let [msb, lsb] = value.to_be_bytes();
        self.bus
            .write(self.address as u8, &[CONFIGURATION, msb, lsb])
            .await
            .map_err(Error::I2c)
    }
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
