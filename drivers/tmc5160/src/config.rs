//! Explicit board characteristics and configuration for external STEP/DIR.
use crate::{ConfigError, registers::*};

/// Board electrical characteristics. There is deliberately no default board.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoardConfig {
    sense_microohms: u32,
    max_rms_milliamps: u32,
}

impl BoardConfig {
    /// BTT TMC5160T Plus V1.0: R5/R15 = 22 mΩ; published ceiling 10.6 A RMS.
    ///
    /// This rating is not a recommended motor current or thermal qualification.
    /// BTT requires active cooling above 3 A. No gate-drive/short-detection values
    /// are guessed: the configuration API preserves those registers unless supplied.
    pub const BTT_TMC5160T_PLUS: Self = Self {
        sense_microohms: 22_000,
        max_rms_milliamps: 10_600,
    };

    /// Describe another board, using nonzero sense resistance and board current ceiling.
    pub const fn new(sense_microohms: u32, max_rms_milliamps: u32) -> Result<Self, ConfigError> {
        if sense_microohms == 0 || max_rms_milliamps == 0 {
            return Err(ConfigError::Board);
        }
        Ok(Self {
            sense_microohms,
            max_rms_milliamps,
        })
    }

    /// Sense resistor resistance, in microohms (22 mΩ = 22,000 µΩ).
    pub const fn sense_microohms(self) -> u32 {
        self.sense_microohms
    }
    /// Published or caller-established board ceiling, not a selected motor current.
    pub const fn max_rms_milliamps(self) -> u32 {
        self.max_rms_milliamps
    }

    /// Nominal full-scale RMS milliamps using the datasheet's typical 325 mV VSRT.
    /// Actual current depends on component tolerances and chopper tuning.
    pub fn full_scale_rms_milliamps(self) -> f64 {
        325_000_000.0 / (core::f64::consts::SQRT_2 * f64::from(self.sense_microohms))
    }

    /// Calculate run/hold scales without exceeding either requested nominal current.
    ///
    /// Currents are RMS milliamps, nonzero, hold <= run. Zero is rejected because
    /// IHOLD=0 means 1/32 scale, not a generic motor-off command. Requests beyond
    /// board or nominal silicon range are rejected, not silently clamped.
    /// Chooses the highest IRUN that can represent both requests; GLOBALSCALER
    /// is rounded down, then IHOLD is rounded down. Reports quantized currents.
    /// Low-current settings can fall outside StealthChop auto-tuning conditions;
    /// current calculation alone does not qualify a motor/chopper combination.
    pub fn current_settings(
        self,
        run_rms_ma: u32,
        hold_rms_ma: u32,
        hold_delay: u8,
    ) -> Result<CurrentSettings, ConfigError> {
        let full = self.full_scale_rms_milliamps();
        if hold_delay > 15
            || hold_rms_ma == 0
            || hold_rms_ma > run_rms_ma
            || run_rms_ma > self.max_rms_milliamps
            || f64::from(run_rms_ma) > full
        {
            return Err(ConfigError::Current);
        }
        for run in (0..=31u8).rev() {
            let per_scale = full * f64::from(run + 1) / 8192.0;
            let scale = ((f64::from(run_rms_ma) / per_scale) as u16).min(256);
            if scale < 32 {
                continue;
            }
            let per_step = full * f64::from(scale) / 8192.0;
            let hold_steps = ((f64::from(hold_rms_ma) / per_step) as u8).min(run + 1);
            if hold_steps == 0 {
                continue;
            }
            return Ok(CurrentSettings {
                board: self,
                scaler: GlobalScaler::new(scale)?,
                currents: IholdIrun::new(hold_steps - 1, run, hold_delay)?,
                run_rms_ma: per_step * f64::from(run + 1),
                hold_rms_ma: per_step * f64::from(hold_steps),
            });
        }
        Err(ConfigError::Current)
    }
}

/// Validated current registers plus nominal quantized currents for the selected board.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurrentSettings {
    pub(crate) board: BoardConfig,
    scaler: GlobalScaler,
    currents: IholdIrun,
    run_rms_ma: f64,
    hold_rms_ma: f64,
}

impl CurrentSettings {
    /// GLOBALSCALER value.
    pub const fn global_scaler(self) -> GlobalScaler {
        self.scaler
    }
    /// IHOLD_IRUN value.
    pub const fn ihold_irun(self) -> IholdIrun {
        self.currents
    }
    /// Nominal run RMS milliamps after quantization, excluding tolerances.
    pub const fn run_rms_milliamps(self) -> f64 {
        self.run_rms_ma
    }
    /// Nominal hold RMS milliamps after quantization, excluding tolerances.
    pub const fn hold_rms_milliamps(self) -> f64 {
        self.hold_rms_ma
    }
}

/// Complete application-selected STEP/DIR configuration.
///
/// Hold DRV_ENN high and stop the pulse source for the entire configuration call.
/// This object has no motor-current default and does not start a pulse source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepDirConfig {
    /// Global mode and DIAG routing.
    pub global: Gconf,
    /// Current settings computed for the driver's board.
    pub current: CurrentSettings,
    /// Desired chopper settings, applied last while DRV_ENN remains high.
    pub chopper: Chopconf,
    /// StealthChop settings. Selecting StealthChop still requires downstream tuning.
    pub pwm: Pwmconf,
    /// StallGuard/CoolStep configuration.
    pub cool: Coolconf,
    /// Standstill delay before hold-current reduction, in units of 2^18 clocks.
    pub power_down: Tpowerdown,
    /// StealthChop threshold; zero disables velocity-based switching.
    pub pwm_threshold: Tpwmthrs,
    /// StallGuard/CoolStep threshold.
    pub cool_threshold: Tcoolthrs,
    /// High-velocity feature threshold.
    pub high_threshold: Thigh,
    /// Optional explicit short-detection tuning; None preserves existing/OTP settings.
    pub short: Option<ShortConf>,
    /// Optional explicit external bridge tuning; None preserves existing/OTP settings.
    pub bridge: Option<DrvConf>,
}

impl StepDirConfig {
    /// Start with SpreadCycle, disabled CoolStep and datasheet PWM settings.
    /// Motor current and chopper configuration must be supplied explicitly.
    pub fn new(current: CurrentSettings, chopper: Chopconf) -> Self {
        Self {
            global: Gconf::STEP_DIR,
            current,
            chopper,
            pwm: Pwmconf::RESET,
            cool: Coolconf::DISABLED,
            power_down: Tpowerdown::from_bits(10).unwrap(),
            pwm_threshold: Tpwmthrs::from_bits(0).unwrap(),
            cool_threshold: Tcoolthrs::from_bits(0).unwrap(),
            high_threshold: Thigh::from_bits(0).unwrap(),
            short: None,
            bridge: None,
        }
    }

    pub(crate) fn validate(&self, board: BoardConfig) -> Result<(), ConfigError> {
        if self.current.board != board {
            return Err(ConfigError::BoardMismatch);
        }
        self.global.validate()?;
        self.chopper.validate()?;
        self.pwm.validate()?;
        self.cool.validate()?;
        self.power_down.validate()?;
        self.pwm_threshold.validate()?;
        self.cool_threshold.validate()?;
        self.high_threshold.validate()?;
        if let Some(v) = self.short {
            v.validate()?;
        }
        if let Some(v) = self.bridge {
            v.validate()?;
        }
        Ok(())
    }
}
