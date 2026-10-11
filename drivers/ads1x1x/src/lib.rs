#![no_std]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod asynchronous;
mod blocking;
pub mod model;

use core::marker::PhantomData;
use model::Model;

const CONVERSION: u8 = 0;
const CONFIG: u8 = 1;
const LOW: u8 = 2;
const HIGH: u8 = 3;
const OS: u16 = 0x8000;
const SINGLE: u16 = 0x0100;

/// Address selected by the ADDR pin (unshifted seven-bit address).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Address {
    /// ADDR connected to ground.
    Ground = 0x48,
    /// ADDR connected to VDD.
    Supply = 0x49,
    /// ADDR connected to SDA.
    Sda = 0x4a,
    /// ADDR connected to SCL.
    Scl = 0x4b,
}

/// Input multiplexer selection. Models ending in 13/14 only support `Differential01`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum Channel {
    /// AIN0 minus AIN1; fixed input on models without a multiplexer.
    #[default]
    Differential01 = 0,
    /// AIN0 minus AIN3.
    Differential03 = 1,
    /// AIN1 minus AIN3.
    Differential13 = 2,
    /// AIN2 minus AIN3.
    Differential23 = 3,
    /// AIN0 relative to ground.
    Single0 = 4,
    /// AIN1 relative to ground.
    Single1 = 5,
    /// AIN2 relative to ground.
    Single2 = 6,
    /// AIN3 relative to ground.
    Single3 = 7,
}

/// Bipolar full-scale range. This does not change the permitted pin voltages.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum FullScaleRange {
    /// ±6.144 V.
    V6_144 = 0,
    /// ±4.096 V.
    V4_096 = 1,
    /// ±2.048 V; the only range on ADS1013/ADS1113.
    #[default]
    V2_048 = 2,
    /// ±1.024 V.
    V1_024 = 3,
    /// ±0.512 V.
    V0_512 = 4,
    /// ±0.256 V.
    V0_256 = 5,
}

impl FullScaleRange {
    /// Positive full-scale boundary in microvolts.
    pub const fn microvolts(self) -> i32 {
        match self {
            Self::V6_144 => 6_144_000,
            Self::V4_096 => 4_096_000,
            Self::V2_048 => 2_048_000,
            Self::V1_024 => 1_024_000,
            Self::V0_512 => 512_000,
            Self::V0_256 => 256_000,
        }
    }
}

/// Conversion operating mode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
    /// Convert repeatedly. Configuration changes can leave one old sample in flight.
    Continuous,
    /// Power down between explicitly triggered conversions.
    #[default]
    SingleShot,
}

/// Number of consecutive out-of-range conversions needed to assert ALERT.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum Queue {
    /// One conversion.
    #[default]
    One = 0,
    /// Two conversions.
    Two = 1,
    /// Four conversions.
    Four = 2,
}

/// Threshold comparator settings; thresholds are provided in signed ADC counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Comparator {
    /// False selects traditional hysteresis; true selects a window comparator.
    pub window: bool,
    /// False selects active-low ALERT; true selects active-high.
    pub active_high: bool,
    /// Hold ALERT until conversion data are read (or an SMBus alert response).
    pub latching: bool,
    /// Required consecutive conversions.
    pub queue: Queue,
}

/// ALERT/RDY function. Unsupported on ADS1013/ADS1113 except `Disabled`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Alert {
    /// High-impedance output; no comparator.
    #[default]
    Disabled,
    /// Program thresholds and enable the comparator.
    Comparator {
        /// Comparator behavior.
        config: Comparator,
        /// Lower threshold in signed ADC counts, inclusive.
        low: i16,
        /// Upper threshold in signed ADC counts, inclusive.
        high: i16,
    },
    /// Configure ALERT/RDY as conversion-ready output.
    Ready {
        /// False selects active-low; true selects active-high.
        active_high: bool,
    },
}

/// Complete configuration with a model-specific data-rate type.
///
/// Defaults match reset settings, excluding OS (which is a command/status bit).
/// Unsupported channels, ranges and ALERT modes are rejected before any bus I/O.
#[derive(Clone, Copy, Debug)]
pub struct Config<M: Model> {
    /// Input selection.
    pub channel: Channel,
    /// ADC scaling range.
    pub range: FullScaleRange,
    /// Single-shot or continuous conversions.
    pub mode: Mode,
    /// Rate from the selected model's 12-bit or 16-bit rate enum.
    pub data_rate: M::DataRate,
    /// Comparator or conversion-ready behavior.
    pub alert: Alert,
}

impl<M: Model> Default for Config<M> {
    fn default() -> Self {
        Self {
            channel: Channel::default(),
            range: FullScaleRange::default(),
            mode: Mode::default(),
            data_rate: M::DataRate::default(),
            alert: Alert::default(),
        }
    }
}

struct Registers {
    bits: u16,
    thresholds: Option<(u16, u16)>,
}

impl<M: Model> Config<M> {
    fn registers<E>(&self) -> Result<Registers, Error<E>> {
        use model::DataRate;
        validate_channel::<M, E>(self.channel)?;
        if !M::HAS_PGA && self.range != FullScaleRange::V2_048 {
            return Err(Error::UnsupportedRange);
        }
        if !M::HAS_ALERT && self.alert != Alert::Disabled {
            return Err(Error::UnsupportedAlert);
        }
        let (alert, thresholds) = match self.alert {
            Alert::Disabled => (3, None),
            Alert::Ready { active_high } => (u16::from(active_high) << 3, Some((0, 0x8000))),
            Alert::Comparator { config, low, high } => {
                if low > high {
                    return Err(Error::InvalidThresholds);
                }
                let thresholds = (
                    encode_threshold::<M, E>(low)?,
                    encode_threshold::<M, E>(high)?,
                );
                let bits = (u16::from(config.window) << 4)
                    | (u16::from(config.active_high) << 3)
                    | (u16::from(config.latching) << 2)
                    | config.queue as u16;
                (bits, Some(thresholds))
            }
        };
        let bits = ((self.channel as u16) << 12)
            | ((self.range as u16) << 9)
            | if self.mode == Mode::SingleShot {
                SINGLE
            } else {
                0
            }
            | ((self.data_rate.bits() as u16) << 5)
            | alert;
        Ok(Registers { bits, thresholds })
    }
}

/// Device, transport or caller configuration error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<E> {
    /// Original I²C error.
    I2c(E),
    /// Selected device has no multiplexer for this input.
    UnsupportedChannel,
    /// Selected device has a fixed ±2.048 V range.
    UnsupportedRange,
    /// Selected device has no ALERT/RDY function.
    UnsupportedAlert,
    /// Thresholds are out of the model's signed range or low exceeds high.
    InvalidThresholds,
    /// Explicit one-shot operations require single-shot mode.
    NotSingleShot,
    /// A conversion is in progress; a new trigger would be ignored by hardware.
    Busy,
    /// A bounded read needs at least one poll.
    InvalidPollLimit,
    /// All polls were used; conversion may still be running.
    Timeout,
}

/// A model-selected ADC on an owned I²C bus, reference or shared-bus handle.
///
/// Use one logical owner per physical ADC. There is no cached configuration.
/// Failed or cancelled writes may still change hardware; recover the bus and
/// reapply the complete configuration. A cancelled trigger may still convert.
pub struct Ads1x1x<BUS, M: Model> {
    bus: BUS,
    address: Address,
    model: PhantomData<M>,
}

impl<BUS, M: Model> Ads1x1x<BUS, M> {
    /// Construct without probing, resetting or configuring the ADC.
    pub const fn new(bus: BUS, address: Address) -> Self {
        Self {
            bus,
            address,
            model: PhantomData,
        }
    }

    /// Release the bus or bus handle.
    pub fn into_inner(self) -> BUS {
        self.bus
    }

    /// Access the bus for caller-managed recovery.
    pub fn bus_mut(&mut self) -> &mut BUS {
        &mut self.bus
    }

    /// Convert signed ADC counts to microvolts, truncating toward zero.
    ///
    /// Pass the range used for this sample. The driver cannot infer the range of
    /// a pending continuous-mode sample after reconfiguration. Input must be a
    /// sample from this model; this method does not validate arbitrary counts.
    pub fn microvolts(counts: i16, range: FullScaleRange) -> i32 {
        (i64::from(counts) * i64::from(range.microvolts()) / (1i64 << (M::BITS - 1))) as i32
    }
}

/// 12-bit, fixed differential input and fixed range; no ALERT/RDY.
pub type Ads1013<BUS> = Ads1x1x<BUS, model::Ads1013>;
/// 12-bit, fixed differential input with PGA and ALERT/RDY.
pub type Ads1014<BUS> = Ads1x1x<BUS, model::Ads1014>;
/// 12-bit, four-input multiplexer with PGA and ALERT/RDY.
pub type Ads1015<BUS> = Ads1x1x<BUS, model::Ads1015>;
/// 16-bit, fixed differential input and fixed range; no ALERT/RDY.
pub type Ads1113<BUS> = Ads1x1x<BUS, model::Ads1113>;
/// 16-bit, fixed differential input with PGA and ALERT/RDY.
pub type Ads1114<BUS> = Ads1x1x<BUS, model::Ads1114>;
/// 16-bit, four-input multiplexer with PGA and ALERT/RDY.
pub type Ads1115<BUS> = Ads1x1x<BUS, model::Ads1115>;

fn validate_channel<M: Model, E>(channel: Channel) -> Result<(), Error<E>> {
    if !M::HAS_MUX && channel != Channel::Differential01 {
        return Err(Error::UnsupportedChannel);
    }
    Ok(())
}

fn encode_threshold<M: Model, E>(counts: i16) -> Result<u16, Error<E>> {
    if M::BITS == 12 && !(-2048..=2047).contains(&counts) {
        return Err(Error::InvalidThresholds);
    }
    Ok((counts as u16) << (16 - M::BITS))
}

fn trigger<E>(raw: u16, channel: Channel) -> Result<u16, Error<E>> {
    if raw & SINGLE == 0 {
        return Err(Error::NotSingleShot);
    }
    if raw & OS == 0 {
        return Err(Error::Busy);
    }
    Ok((raw & !0x7000) | ((channel as u16) << 12) | OS)
}

fn ready<E>(raw: u16) -> Result<bool, Error<E>> {
    if raw & SINGLE == 0 {
        return Err(Error::NotSingleShot);
    }
    Ok(raw & OS != 0)
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
