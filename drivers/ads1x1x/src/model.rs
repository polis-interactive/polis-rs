//! Sealed device models and their supported conversion rates.
mod sealed {
    pub trait Sealed {}
}

/// A supported conversion-rate encoding; implemented only by this crate.
pub trait DataRate: sealed::Sealed + Copy + Default + core::fmt::Debug {
    /// Three-bit hardware encoding.
    fn bits(self) -> u8;
}

/// A supported physical ADC; implementations are sealed to the six tested models.
pub trait Model: sealed::Sealed + Copy + core::fmt::Debug {
    /// Model-specific conversion rates.
    type DataRate: DataRate;
    /// Signed conversion resolution.
    const BITS: u8;
    /// Whether the input multiplexer is present.
    const HAS_MUX: bool;
    /// Whether the range is programmable.
    const HAS_PGA: bool;
    /// Whether comparator and conversion-ready output are present.
    const HAS_ALERT: bool;
}

/// Conversion rates for 12-bit models.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum DataRate12Bit {
    /// 128 samples per second.
    Sps128 = 0,
    /// 250 samples per second.
    Sps250 = 1,
    /// 490 samples per second.
    Sps490 = 2,
    /// 920 samples per second.
    Sps920 = 3,
    /// 1600 samples per second.
    #[default]
    Sps1600 = 4,
    /// 2400 samples per second.
    Sps2400 = 5,
    /// 3300 samples per second.
    Sps3300 = 6,
}
impl sealed::Sealed for DataRate12Bit {}
impl DataRate for DataRate12Bit {
    fn bits(self) -> u8 {
        self as u8
    }
}

/// Conversion rates for 16-bit models.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum DataRate16Bit {
    /// 8 samples per second.
    Sps8 = 0,
    /// 16 samples per second.
    Sps16 = 1,
    /// 32 samples per second.
    Sps32 = 2,
    /// 64 samples per second.
    Sps64 = 3,
    /// 128 samples per second.
    #[default]
    Sps128 = 4,
    /// 250 samples per second.
    Sps250 = 5,
    /// 475 samples per second.
    Sps475 = 6,
    /// 860 samples per second.
    Sps860 = 7,
}
impl sealed::Sealed for DataRate16Bit {}
impl DataRate for DataRate16Bit {
    fn bits(self) -> u8 {
        self as u8
    }
}

/// ADS1013 model selection.
#[derive(Clone, Copy, Debug)]
pub struct Ads1013;
impl sealed::Sealed for Ads1013 {}
impl Model for Ads1013 {
    type DataRate = DataRate12Bit;
    const BITS: u8 = 12;
    const HAS_MUX: bool = false;
    const HAS_PGA: bool = false;
    const HAS_ALERT: bool = false;
}

/// ADS1014 model selection.
#[derive(Clone, Copy, Debug)]
pub struct Ads1014;
impl sealed::Sealed for Ads1014 {}
impl Model for Ads1014 {
    type DataRate = DataRate12Bit;
    const BITS: u8 = 12;
    const HAS_MUX: bool = false;
    const HAS_PGA: bool = true;
    const HAS_ALERT: bool = true;
}

/// ADS1015 model selection.
#[derive(Clone, Copy, Debug)]
pub struct Ads1015;
impl sealed::Sealed for Ads1015 {}
impl Model for Ads1015 {
    type DataRate = DataRate12Bit;
    const BITS: u8 = 12;
    const HAS_MUX: bool = true;
    const HAS_PGA: bool = true;
    const HAS_ALERT: bool = true;
}

/// ADS1113 model selection.
#[derive(Clone, Copy, Debug)]
pub struct Ads1113;
impl sealed::Sealed for Ads1113 {}
impl Model for Ads1113 {
    type DataRate = DataRate16Bit;
    const BITS: u8 = 16;
    const HAS_MUX: bool = false;
    const HAS_PGA: bool = false;
    const HAS_ALERT: bool = false;
}

/// ADS1114 model selection.
#[derive(Clone, Copy, Debug)]
pub struct Ads1114;
impl sealed::Sealed for Ads1114 {}
impl Model for Ads1114 {
    type DataRate = DataRate16Bit;
    const BITS: u8 = 16;
    const HAS_MUX: bool = false;
    const HAS_PGA: bool = true;
    const HAS_ALERT: bool = true;
}

/// ADS1115 model selection.
#[derive(Clone, Copy, Debug)]
pub struct Ads1115;
impl sealed::Sealed for Ads1115 {}
impl Model for Ads1115 {
    type DataRate = DataRate16Bit;
    const BITS: u8 = 16;
    const HAS_MUX: bool = true;
    const HAS_PGA: bool = true;
    const HAS_ALERT: bool = true;
}
