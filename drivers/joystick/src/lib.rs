//! Portable joystick conditioning with replaceable async ADC sources.
//! No board pins, buttons, runtime selection, or product calibration defaults.
#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(all(feature = "rp2040", feature = "rp235xa"))]
compile_error!("select only one MCU feature: rp2040 or rp235xa");

#[cfg(feature = "ads1x1x")]
pub mod ads;
#[cfg(feature = "rp-adc")]
pub mod rp;

/// Read the two axes from an owned or borrowed ADC. Values use source-native counts.
#[allow(async_fn_in_trait)]
pub trait AxisSource {
    /// Acquisition error; callers own deadlines and recovery.
    type Error;
    /// Return a complete X/Y pair or an error. Never report a partial pair as fresh.
    async fn read_axes(&mut self) -> Result<[i16; 2], Self::Error>;
}

/// Validated per-axis calibration, in native ADC counts.
#[derive(Clone, Copy, Debug)]
pub struct Calibration {
    minimum: i16,
    center: i16,
    maximum: i16,
    deadzone: i16,
    inverted: bool,
}

impl Calibration {
    /// Require min < center < max and a nonnegative deadzone smaller than both spans.
    pub fn new(
        minimum: i16,
        center: i16,
        maximum: i16,
        deadzone: i16,
        inverted: bool,
    ) -> Option<Self> {
        let left = i32::from(center) - i32::from(minimum);
        let right = i32::from(maximum) - i32::from(center);
        (deadzone >= 0 && left > i32::from(deadzone) && right > i32::from(deadzone)).then_some(
            Self {
                minimum,
                center,
                maximum,
                deadzone,
                inverted,
            },
        )
    }

    /// Piecewise-linear, clamped normalization to -32767..32767, zero in the deadzone.
    pub fn normalize(self, raw: i16) -> i16 {
        let offset = i32::from(raw) - i32::from(self.center);
        let magnitude = offset.abs() - i32::from(self.deadzone);
        if magnitude <= 0 {
            return 0;
        }
        let span = if offset < 0 {
            i32::from(self.center) - i32::from(self.minimum)
        } else {
            i32::from(self.maximum) - i32::from(self.center)
        } - i32::from(self.deadzone);
        let value = magnitude.min(span) * 32767 / span;
        let value = if offset < 0 { -value } else { value };
        (if self.inverted { -value } else { value }) as i16
    }
}

/// One complete joystick acquisition, useful for both monitoring and profile mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    /// Source-native X/Y counts, retained for calibration and diagnosis.
    pub raw: [i16; 2],
    /// Calibrated X/Y values in -32767..32767.
    pub position: [i16; 2],
}

/// Joystick policy is independent of ADC ownership. Pass a source at sampling time,
/// allowing two joysticks to borrow the same four-channel ADC sequentially.
pub struct Joystick {
    calibration: [Calibration; 2],
}
impl Joystick {
    /// Construct with explicit per-axis calibration, without performing I/O.
    pub const fn new(calibration: [Calibration; 2]) -> Self {
        Self { calibration }
    }

    /// Acquire and normalize a pair. Failed reads never fabricate a fresh reading.
    pub async fn sample<S: AxisSource>(&self, source: &mut S) -> Result<Reading, S::Error> {
        let raw = source.read_axes().await?;
        Ok(Reading {
            raw,
            position: core::array::from_fn(|i| self.calibration[i].normalize(raw[i])),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_handles_asymmetry_deadzone_clipping_and_inversion() {
        let c = Calibration::new(100, 800, 1600, 20, false).unwrap();
        for raw in [780, 800, 820] {
            assert_eq!(c.normalize(raw), 0);
        }
        assert_eq!(c.normalize(100), -32767);
        assert_eq!(c.normalize(1600), 32767);
        assert_eq!(c.normalize(i16::MIN), -32767);
        assert_eq!(c.normalize(i16::MAX), 32767);
        let flipped = Calibration::new(100, 800, 1600, 20, true).unwrap();
        assert_eq!(flipped.normalize(100), 32767);
        for args in [(0, 0, 10, 0), (0, 5, 10, 5), (10, 5, 0, 0), (0, 5, 10, -1)] {
            assert!(Calibration::new(args.0, args.1, args.2, args.3, false).is_none());
        }
    }

    #[test]
    fn integer_rounding_and_full_i16_count_range_are_preserved() {
        let c = Calibration::new(100, 800, 1600, 20, false).unwrap();
        // Division truncates magnitude before applying the sign; no rounding
        // to nearest and no -32768 output are introduced by the extraction.
        assert_eq!(c.normalize(440), -16383);
        assert_eq!(c.normalize(1210), 16383);
        assert_eq!(c.normalize(779), -48);
        assert_eq!(c.normalize(821), 42);
        let wide = Calibration::new(i16::MIN, 0, i16::MAX, 0, false).unwrap();
        assert_eq!(wide.normalize(i16::MIN), -32767);
        assert_eq!(wide.normalize(i16::MAX), 32767);
        assert_eq!(wide.normalize(-1), 0);
        assert_eq!(wide.normalize(1), 1);
    }

    struct Source(Result<[i16; 2], ()>);
    impl AxisSource for Source {
        type Error = ();
        async fn read_axes(&mut self) -> Result<[i16; 2], ()> {
            self.0
        }
    }

    #[tokio::test]
    async fn interchangeable_sources_preserve_raw_values_and_errors() {
        let joystick = Joystick::new([Calibration::new(0, 825, 1650, 20, false).unwrap(); 2]);
        let reading = joystick.sample(&mut Source(Ok([0, 1650]))).await.unwrap();
        assert_eq!(reading.raw, [0, 1650]);
        assert_eq!(reading.position, [-32767, 32767]);
        assert_eq!(joystick.sample(&mut Source(Err(()))).await, Err(()));
    }
}
