//! Borrowed polis-ads1x1x adapter; the caller configures the physical ADC once.
use crate::AxisSource;
use embedded_hal_async::{delay::DelayNs, i2c::I2c};
use polis_ads1x1x::{Ads1x1x, Channel, Error, model::Model};

/// Two selected inputs on a shared ADS1x1x. Supports both ADS1015 and ADS1115.
pub struct AdsAxes<'a, B, M: Model, D> {
    adc: &'a mut Ads1x1x<B, M>,
    delay: &'a mut D,
    channels: [Channel; 2],
}
impl<'a, B, M: Model, D> AdsAxes<'a, B, M, D> {
    /// Borrow an ADC configured for single-shot mode. Channels define X then Y.
    pub fn new(adc: &'a mut Ads1x1x<B, M>, delay: &'a mut D, channels: [Channel; 2]) -> Self {
        Self {
            adc,
            delay,
            channels,
        }
    }
}
impl<B: I2c, M: Model, D: DelayNs> AxisSource for AdsAxes<'_, B, M, D> {
    type Error = Error<B::Error>;
    async fn read_axes(&mut self) -> Result<[i16; 2], Self::Error> {
        let x = self
            .adc
            .read_one_shot_async(self.channels[0], self.delay, 200)
            .await?;
        let y = self
            .adc
            .read_one_shot_async(self.channels[1], self.delay, 200)
            .await?;
        Ok([x, y])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Calibration, Joystick};
    use embedded_hal_async::i2c::{ErrorKind, ErrorType, Operation};
    use polis_ads1x1x::{Address, Ads1015, Config, FullScaleRange};

    struct Bus {
        config: u16,
        triggered: [u8; 4],
        count: usize,
        fail_channel: Option<u8>,
    }
    impl ErrorType for Bus {
        type Error = ErrorKind;
    }
    impl I2c for Bus {
        async fn transaction(
            &mut self,
            address: u8,
            ops: &mut [Operation<'_>],
        ) -> Result<(), ErrorKind> {
            tokio::task::yield_now().await;
            assert_eq!(address, 0x48);
            match ops {
                [Operation::Write(bytes)] => {
                    assert_eq!(bytes[0], 1);
                    self.config = u16::from_be_bytes([bytes[1], bytes[2]]);
                    if self.config & 0x8000 != 0 {
                        self.triggered[self.count] = ((self.config >> 12) & 7) as u8 - 4;
                        self.count += 1;
                    }
                }
                [Operation::Write(register), Operation::Read(output)] => {
                    let value = match register[0] {
                        1 => self.config | 0x8000,
                        0 => {
                            let channel = ((self.config >> 12) & 7) as u8 - 4;
                            if self.fail_channel == Some(channel) {
                                return Err(ErrorKind::Bus);
                            }
                            let values = [-1i16, 825, 1650, 400];
                            (values[usize::from(channel)] << 4) as u16
                        }
                        _ => panic!("unexpected register"),
                    };
                    output.copy_from_slice(&value.to_be_bytes());
                }
                _ => panic!("unexpected I2C operation"),
            }
            Ok(())
        }
    }
    struct Delay;
    impl DelayNs for Delay {
        async fn delay_ns(&mut self, _: u32) {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn two_joysticks_share_one_adc_and_preserve_channel_order_and_range() {
        let bus = Bus {
            config: 0,
            triggered: [0; 4],
            count: 0,
            fail_channel: None,
        };
        let mut adc = Ads1015::new(bus, Address::Ground);
        adc.configure_async(Config {
            range: FullScaleRange::V4_096,
            ..Default::default()
        })
        .await
        .unwrap();
        let joysticks = core::array::from_fn::<_, 2, _>(|_| {
            Joystick::new([Calibration::new(0, 825, 1650, 20, false).unwrap(); 2])
        });
        let first = joysticks[0]
            .sample(&mut AdsAxes::new(
                &mut adc,
                &mut Delay,
                [Channel::Single0, Channel::Single1],
            ))
            .await
            .unwrap();
        let second = joysticks[1]
            .sample(&mut AdsAxes::new(
                &mut adc,
                &mut Delay,
                [Channel::Single2, Channel::Single3],
            ))
            .await
            .unwrap();
        assert_eq!(first.raw, [-1, 825]);
        assert_eq!(first.position, [-32767, 0]);
        assert_eq!(second.raw, [1650, 400]);
        assert_eq!(second.position[0], 32767);
        let bus = adc.into_inner();
        assert_eq!(bus.triggered, [0, 1, 2, 3]);
        assert_eq!((bus.config >> 9) & 7, FullScaleRange::V4_096 as u16);
    }

    #[tokio::test]
    async fn failed_second_axis_does_not_return_partial_reading() {
        let bus = Bus {
            config: 0x8103,
            triggered: [0; 4],
            count: 0,
            fail_channel: Some(1),
        };
        let mut adc = Ads1015::new(bus, Address::Ground);
        let joystick = Joystick::new([Calibration::new(0, 825, 1650, 0, false).unwrap(); 2]);
        let result = joystick
            .sample(&mut AdsAxes::new(
                &mut adc,
                &mut Delay,
                [Channel::Single0, Channel::Single1],
            ))
            .await;
        assert!(matches!(result, Err(Error::I2c(ErrorKind::Bus))));
        assert_eq!(adc.into_inner().count, 2);
    }

    #[tokio::test]
    async fn failed_first_axis_does_not_attempt_second_axis() {
        let bus = Bus {
            config: 0x8103,
            triggered: [0; 4],
            count: 0,
            fail_channel: Some(0),
        };
        let mut adc = Ads1015::new(bus, Address::Ground);
        let joystick = Joystick::new([Calibration::new(0, 825, 1650, 0, false).unwrap(); 2]);
        let result = joystick
            .sample(&mut AdsAxes::new(
                &mut adc,
                &mut Delay,
                [Channel::Single0, Channel::Single1],
            ))
            .await;
        assert!(matches!(result, Err(Error::I2c(ErrorKind::Bus))));
        assert_eq!(adc.into_inner().count, 1);
    }
}
