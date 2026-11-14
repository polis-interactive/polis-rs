//! TMC5160/A register values for external STEP/DIR operation.
//!
//! Complete writes avoid fictitious readback of write-only registers. `from_bits`
//! rejects reserved bits and unsupported field encodings. It does not establish
//! that a setting is electrically appropriate for a particular motor or bridge.
//! Reads retain all returned bits for diagnostics; writing them validates again.

use crate::ConfigError;

mod sealed {
    pub trait Register {}
}

/// A supported register. Sealed to keep motion, UART and OTP writes out of this API.
pub trait Register: sealed::Register + Copy {
    /// Seven-bit register address.
    const ADDRESS: u8;
}

/// A register that can actually be read from the chip.
pub trait Readable: Register {
    /// Decode a hardware response, preserving reserved bits for inspection.
    fn from_read(bits: u32) -> Self;
}

/// A writable register value, revalidated before any bus operation.
pub trait Writable: Register {
    /// Raw register value.
    fn bits(self) -> u32;
    /// Check reserved bits and supported field encodings.
    fn validate(self) -> Result<(), ConfigError>;
}

macro_rules! register {
    ($(#[$doc:meta])* $name:ident, $address:expr) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct $name(u32);
        impl $name {
            /// Raw bits, including any reserved bits returned by hardware.
            pub const fn bits(self) -> u32 { self.0 }
        }
        impl sealed::Register for $name {}
        impl Register for $name { const ADDRESS: u8 = $address; }
    };
}
macro_rules! readable {
    ($($name:ident),+ $(,)?) => {$(
        impl Readable for $name {
            fn from_read(bits: u32) -> Self { Self(bits) }
        }
    )+};
}
macro_rules! writable {
    ($name:ident, $mask:expr, $check:expr) => {
        impl $name {
            /// Construct a complete value, rejecting reserved or unsupported fields.
            pub fn from_bits(bits: u32) -> Result<Self, ConfigError> {
                let value = Self(bits);
                value.validate()?;
                Ok(value)
            }
        }
        impl Writable for $name {
            fn bits(self) -> u32 {
                self.0
            }
            fn validate(self) -> Result<(), ConfigError> {
                if self.0 & !$mask != 0 {
                    return Err(ConfigError::ReservedBits {
                        address: Self::ADDRESS,
                    });
                }
                if !($check)(self.0) {
                    return Err(ConfigError::InvalidField {
                        address: Self::ADDRESS,
                    });
                }
                Ok(())
            }
        }
    };
}

register!(/// GCONF: global configuration and DIAG routing (direct/test modes excluded).
    Gconf, 0x00);
register!(/// GSTAT: latched reset, driver-error and charge-pump undervoltage flags.
    Gstat, 0x01);
register!(/// IOIN: input pin levels and silicon version.
    Ioin, 0x04);
register!(/// SHORT_CONF: short-detection thresholds, filter and delay; write-only.
    ShortConf, 0x09);
register!(/// DRV_CONF: external MOSFET gate drive, dead time and sense filtering; write-only.
    DrvConf, 0x0a);
register!(/// GLOBALSCALER: 32..=256 current scale; 256 is encoded as zero; write-only.
    GlobalScaler, 0x0b);
register!(/// IHOLD_IRUN: hold/run current scales and reduction delay; write-only.
    IholdIrun, 0x10);
register!(/// TPOWERDOWN: standstill power-down delay in units of 2^18 clocks; write-only.
    Tpowerdown, 0x11);
register!(/// TSTEP: measured step interval, not an executed-pulse counter.
    Tstep, 0x12);
register!(/// TPWMTHRS: StealthChop upper-velocity threshold in clock ticks; write-only.
    Tpwmthrs, 0x13);
register!(/// TCOOLTHRS: StallGuard/CoolStep lower-velocity threshold; write-only.
    Tcoolthrs, 0x14);
register!(/// THIGH: upper-velocity threshold for current/chopper features; write-only.
    Thigh, 0x15);
register!(/// MSCNT: position within the microstep table, not total motion.
    Mscnt, 0x6a);
register!(/// MSCURACT: signed unscaled microstep currents for phases A/B.
    Mscuract, 0x6b);
register!(/// CHOPCONF: microsteps, interpolation, chopper timing and protection.
    Chopconf, 0x6c);
register!(/// COOLCONF: CoolStep and StallGuard2 tuning; write-only.
    Coolconf, 0x6d);
register!(/// DRV_STATUS: load, current scale, standstill and fault diagnostics.
    DrvStatus, 0x6f);
register!(/// PWMCONF: StealthChop configuration; write-only.
    Pwmconf, 0x70);
register!(/// PWM_SCALE: automatic PWM amplitude regulation result.
    PwmScale, 0x71);
register!(/// PWM_AUTO: automatically determined PWM offset and gradient.
    PwmAuto, 0x72);

readable!(
    Gconf, Gstat, Ioin, Tstep, Mscnt, Mscuract, Chopconf, DrvStatus, PwmScale, PwmAuto
);
writable!(Gconf, 0x0000_ffff, |_| true);
// GSTAT intentionally has no Writable implementation: use clear_status explicitly.
writable!(ShortConf, 0x0007_0f0f, |v: u32| v & 15 >= 4
    && (v >> 8) & 15 >= 2);
writable!(DrvConf, 0x003f_0f1f, |v: u32| v & 31 <= 24);
writable!(GlobalScaler, 0xff, |v: u32| v == 0 || v >= 32);
writable!(IholdIrun, 0x000f_1f1f, |_| true);
writable!(Tpowerdown, 0xff, |_| true);
writable!(Tpwmthrs, 0x000f_ffff, |_| true);
writable!(Tcoolthrs, 0x000f_ffff, |_| true);
writable!(Thigh, 0x000f_ffff, |_| true);
writable!(Chopconf, 0xfffd_dfff, |v: u32| {
    let toff = v & 15;
    let tbl = (v >> 15) & 3;
    let constant_off_time = v & (1 << 14) != 0;
    (v >> 24) & 15 <= 8
        && (toff != 1 || tbl >= 2)
        && (constant_off_time || ((v >> 7) & 15) + ((v >> 4) & 7) <= 18)
});
writable!(Coolconf, 0x017f_ef6f, |_| true);
writable!(Pwmconf, 0xff3f_ffff, |v: u32| {
    (v & (1 << 19) == 0 || v & (1 << 18) != 0) && (v & (1 << 18) == 0 || (v >> 24) & 15 != 0)
});

impl Gconf {
    /// External STEP/DIR, input filtering, SpreadCycle, open-drain DIAG outputs.
    pub const STEP_DIR: Self = Self(8);
    /// Select StealthChop (`true`) or SpreadCycle (`false`). Configure while disabled.
    pub const fn with_stealth_chop(self, enabled: bool) -> Self {
        Self((self.0 & !4) | ((enabled as u32) << 2))
    }
    /// Invert the external direction input.
    pub const fn with_shaft_inverted(self, inverted: bool) -> Self {
        Self((self.0 & !16) | ((inverted as u32) << 4))
    }
    /// Configure DIAG routing and electrical output mode using bits 5..=13.
    pub fn with_diag_bits(self, bits: u32) -> Result<Self, ConfigError> {
        if bits & !0x3fe0 != 0 {
            return Err(ConfigError::InvalidField {
                address: Self::ADDRESS,
            });
        }
        Self::from_bits((self.0 & !0x3fe0) | bits)
    }
}

impl Gstat {
    /// Flags selected for explicit write-one-to-clear acknowledgement.
    pub const fn clear_flags(reset: bool, driver_error: bool, undervoltage: bool) -> Self {
        Self(reset as u32 | ((driver_error as u32) << 1) | ((undervoltage as u32) << 2))
    }
    /// A reset was latched; previous configuration must not be assumed valid.
    pub const fn reset(self) -> bool {
        self.0 & 1 != 0
    }
    /// Driver shutdown was latched; inspect DRV_STATUS before acknowledging.
    pub const fn driver_error(self) -> bool {
        self.0 & 2 != 0
    }
    /// Charge-pump undervoltage was latched.
    pub const fn undervoltage(self) -> bool {
        self.0 & 4 != 0
    }
}

impl Ioin {
    /// Silicon version byte; the documented TMC5160/A value is 0x30.
    pub const fn version(self) -> u8 {
        (self.0 >> 24) as u8
    }
    /// SD_MODE is high: the chip accepts external STEP/DIR.
    pub const fn external_step_dir(self) -> bool {
        self.0 & 0x40 != 0
    }
    /// DRV_ENN is high: external power-stage disable is asserted.
    pub const fn driver_disabled(self) -> bool {
        self.0 & 0x10 != 0
    }
}

impl GlobalScaler {
    /// Construct a scale in 32..=256; 256 uses the special zero encoding.
    pub fn new(scale: u16) -> Result<Self, ConfigError> {
        if !(32..=256).contains(&scale) {
            return Err(ConfigError::InvalidField {
                address: Self::ADDRESS,
            });
        }
        Ok(Self(u32::from(scale) & 0xff))
    }
    /// Effective scale, decoding zero as 256.
    pub const fn scale(self) -> u16 {
        if self.0 == 0 { 256 } else { self.0 as u16 }
    }
}

impl IholdIrun {
    /// Raw 5-bit current scales (zero means 1/32, not off) and 4-bit delay.
    pub fn new(hold: u8, run: u8, delay: u8) -> Result<Self, ConfigError> {
        if hold > 31 || run > 31 || delay > 15 {
            return Err(ConfigError::InvalidField {
                address: Self::ADDRESS,
            });
        }
        Ok(Self(
            u32::from(hold) | (u32::from(run) << 8) | (u32::from(delay) << 16),
        ))
    }
    /// Raw hold-current scale.
    pub const fn hold(self) -> u8 {
        (self.0 & 31) as u8
    }
    /// Raw run-current scale.
    pub const fn run(self) -> u8 {
        ((self.0 >> 8) & 31) as u8
    }
}

/// Number of microsteps per full step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Microsteps {
    /// 256 microsteps.
    M256 = 0,
    /// 128 microsteps.
    M128 = 1,
    /// 64 microsteps.
    M64 = 2,
    /// 32 microsteps.
    M32 = 3,
    /// 16 microsteps.
    M16 = 4,
    /// 8 microsteps.
    M8 = 5,
    /// 4 microsteps.
    M4 = 6,
    /// 2 microsteps.
    M2 = 7,
    /// Full steps.
    Full = 8,
}

impl Chopconf {
    /// Datasheet reset baseline (TOFF=0, outputs off); not a tuned motor profile.
    pub const DISABLED: Self = Self(0x1041_0150);
    /// Set resolution and interpolation, retaining other fields.
    pub const fn with_microsteps(self, resolution: Microsteps, interpolate: bool) -> Self {
        Self((self.0 & !0x1f00_0000) | ((resolution as u32) << 24) | ((interpolate as u32) << 28))
    }
    /// Select off time (0 disables the bridges). Validates its blank-time constraint.
    pub fn with_off_time(self, off_time: u8) -> Result<Self, ConfigError> {
        if off_time > 15 {
            return Err(ConfigError::InvalidField {
                address: Self::ADDRESS,
            });
        }
        Self::from_bits((self.0 & !15) | u32::from(off_time))
    }
    /// Clear TOFF while retaining all other settings.
    pub const fn disabled(self) -> Self {
        Self(self.0 & !15)
    }
}

impl Coolconf {
    /// CoolStep disabled and StallGuard threshold zero, with no filtering.
    pub const DISABLED: Self = Self(0);
    /// Set signed StallGuard sensitivity and optional four-fullstep filtering.
    pub fn with_stall_guard(self, threshold: i8, filtered: bool) -> Result<Self, ConfigError> {
        if !(-64..=63).contains(&threshold) {
            return Err(ConfigError::InvalidField {
                address: Self::ADDRESS,
            });
        }
        Self::from_bits(
            (self.0 & !0x017f_0000)
                | (((threshold as u8 & 0x7f) as u32) << 16)
                | ((filtered as u32) << 24),
        )
    }
}

impl Pwmconf {
    /// Datasheet reset settings. StealthChop tuning still requires application-controlled motion.
    pub const RESET: Self = Self(0xc40c_001e);
}

impl DrvStatus {
    /// StallGuard load result; not valid as a stall measurement in StealthChop/standstill.
    pub const fn stall_guard_result(self) -> u16 {
        (self.0 & 0x3ff) as u16
    }
    /// Actual raw current scale.
    pub const fn current_scale(self) -> u8 {
        ((self.0 >> 16) & 31) as u8
    }
    /// Standstill detected by the chip.
    pub const fn standstill(self) -> bool {
        self.0 & (1 << 31) != 0
    }
    /// StealthChop currently active.
    pub const fn stealth_chop(self) -> bool {
        self.0 & (1 << 14) != 0
    }
    /// Thermal shutdown threshold reached.
    pub const fn overtemperature(self) -> bool {
        self.0 & (1 << 25) != 0
    }
    /// Thermal prewarning threshold reached.
    pub const fn temperature_warning(self) -> bool {
        self.0 & (1 << 26) != 0
    }
    /// Phase A/B short-to-ground flags, respectively in bits 0/1.
    pub const fn short_to_ground(self) -> u8 {
        ((self.0 >> 27) & 3) as u8
    }
    /// Phase A/B short-to-supply flags, respectively in bits 0/1.
    pub const fn short_to_supply(self) -> u8 {
        ((self.0 >> 12) & 3) as u8
    }
    /// Phase A/B open-load flags; may be misleading at standstill or high speed.
    pub const fn open_load(self) -> u8 {
        ((self.0 >> 29) & 3) as u8
    }
    /// StallGuard stall indication.
    pub const fn stalled(self) -> bool {
        self.0 & (1 << 24) != 0
    }
}
