use super::*;
use embedded_hal_async::{delay::DelayNs, i2c::I2c};

impl<BUS: I2c, M: Model> Ads1x1x<BUS, M> {
    /// Apply complete settings without triggering a single-shot conversion.
    ///
    /// With ALERT enabled this first disables the comparator, writes both
    /// thresholds, then enables the requested function. These writes are not
    /// atomic; after an error, reapply the full configuration. Continuous mode
    /// starts converting immediately and can still return a previous sample.
    pub async fn configure_async(&mut self, config: Config<M>) -> Result<(), Error<BUS::Error>> {
        let Registers { bits, thresholds } = config.registers()?;
        if let Some((low, high)) = thresholds {
            self.write_register_async(CONFIG, bits | 3).await?;
            self.write_register_async(LOW, low).await?;
            self.write_register_async(HIGH, high).await?;
        }
        self.write_register_async(CONFIG, bits).await
    }

    /// Read the latest result as signed ADC counts. Does not wait or trigger.
    ///
    /// ADS101x samples are sign-extended from 12 bits; ADS111x use all 16 bits.
    /// Reading also clears a latched comparator assertion.
    pub async fn read_conversion_async(&mut self) -> Result<i16, Error<BUS::Error>> {
        Ok((self.read_register_async(CONVERSION).await? as i16) >> (16 - M::BITS))
    }

    /// Read the raw configuration/status register for inspection or recovery.
    pub async fn configuration_bits_async(&mut self) -> Result<u16, Error<BUS::Error>> {
        self.read_register_async(CONFIG).await
    }

    /// Trigger one conversion in single-shot mode, preserving other settings.
    ///
    /// Checks the current hardware mode/status, rather than cached state. Returns
    /// `Busy` if converting. Wait for power-up (approximately 25 µs per TI) before
    /// polling OS; the bounded helper uses a 1 ms delay before its first poll.
    /// A successful write or a cancelled/failed transfer can start a conversion.
    pub async fn start_one_shot_async(
        &mut self,
        channel: Channel,
    ) -> Result<(), Error<BUS::Error>> {
        validate_channel::<M, BUS::Error>(channel)?;
        let raw = self.read_register_async(CONFIG).await?;
        self.write_register_async(CONFIG, trigger::<BUS::Error>(raw, channel)?)
            .await
    }

    /// Poll once for single-shot idle/completion; this method never loops.
    ///
    /// True also means idle before any trigger. Use after `start_one_shot_async` and a
    /// startup delay; impose a caller deadline when polling manually.
    pub async fn one_shot_ready_async(&mut self) -> Result<bool, Error<BUS::Error>> {
        ready(self.read_register_async(CONFIG).await?)
    }

    /// Trigger, poll at 1 ms intervals up to `poll_limit` times, then read.
    ///
    /// Requires single-shot mode. Zero polls is invalid and performs no I/O.
    /// Use e.g. 200 polls to accommodate the slowest (8 SPS) rate. The bound
    /// covers polling only: the bus implementation must enforce I²C timeouts.
    /// Timeout leaves a possible conversion running; poll or reconfigure before
    /// retrying. Use the split methods for higher-throughput or GPIO-driven I/O.
    pub async fn read_one_shot_async(
        &mut self,
        channel: Channel,
        delay: &mut impl DelayNs,
        poll_limit: u16,
    ) -> Result<i16, Error<BUS::Error>> {
        if poll_limit == 0 {
            return Err(Error::InvalidPollLimit);
        }
        self.start_one_shot_async(channel).await?;
        for _ in 0..poll_limit {
            delay.delay_ms(1).await;
            if self.one_shot_ready_async().await? {
                return self.read_conversion_async().await;
            }
        }
        Err(Error::Timeout)
    }

    async fn read_register_async(&mut self, register: u8) -> Result<u16, Error<BUS::Error>> {
        let mut bytes = [0; 2];
        self.bus
            .write_read(self.address as u8, &[register], &mut bytes)
            .await
            .map_err(Error::I2c)?;
        Ok(u16::from_be_bytes(bytes))
    }

    async fn write_register_async(
        &mut self,
        register: u8,
        value: u16,
    ) -> Result<(), Error<BUS::Error>> {
        let [hi, lo] = value.to_be_bytes();
        self.bus
            .write(self.address as u8, &[register, hi, lo])
            .await
            .map_err(Error::I2c)
    }
}
