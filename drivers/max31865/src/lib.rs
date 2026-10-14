#![no_std]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

// Adapted from broose-goose/adafruit-max31865-rs and Adafruit_MAX31865.
// See PROVENANCE.md and NOTICE for attribution and retained upstream notices.

use embedded_hal_async::{delay::DelayNs, spi::SpiDevice};

const CONFIG: u8 = 0x00;
const RTD: u8 = 0x01;
const HIGH_THRESHOLD: u8 = 0x03;
const LOW_THRESHOLD: u8 = 0x05;
const FAULT: u8 = 0x07;
const BIAS: u8 = 0x80;
const AUTO: u8 = 0x40;
const ONE_SHOT: u8 = 0x20;
const CLEAR_FAULT: u8 = 0x02;
// Only persistent controls; never echo one-shot, fault-cycle or clear triggers.
const PERSISTENT: u8 = 0xd1;

/// RTD lead configuration; board jumpers must match the selected wiring.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WireCount {
    /// Two leads; lead resistance contributes to the measurement.
    Two,
    /// Three leads with matched lead resistance compensation.
    Three,
    /// Four leads with separate force and sense connections.
    Four,
}

/// Mains rejection filter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterHz {
    /// 50 Hz rejection; allow 66 ms for a one-shot conversion.
    Hz50,
    /// 60 Hz rejection; allow 55 ms for a one-shot conversion.
    Hz60,
}

/// Fault detection command. Reading latched faults is a separate operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FaultCycle {
    /// Automatic timing, only for input RC time constants at most 100 µs.
    Auto,
    /// Start manual detection and open the FORCE- switch.
    ManualRun,
    /// Finish manual detection after caller-managed input settling.
    ManualFinish,
}

bitflags::bitflags! {
    /// Latched fault register flags; several faults can be present together.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct FaultStatus: u8 {
        /// RTD measurement reached the high threshold.
        const HIGHTHRESH = 0x80;
        /// RTD measurement reached the low threshold.
        const LOWTHRESH = 0x40;
        /// REFIN- is greater than 85% of bias voltage.
        const REFINLOW = 0x20;
        /// REFIN- is below 85% of bias with FORCE- open.
        const REFINHIGH = 0x10;
        /// RTDIN- is below 85% of bias with FORCE- open.
        const RTDINLOW = 0x08;
        /// Under- or overvoltage on a protected input.
        const OVUV = 0x04;
    }
}

/// Description of the first set fault bit; iterate flags to describe every fault.
pub fn fault_message(fault: FaultStatus) -> &'static str {
    if fault.contains(FaultStatus::HIGHTHRESH) {
        "RTD High Threshold"
    } else if fault.contains(FaultStatus::LOWTHRESH) {
        "RTD Low Threshold"
    } else if fault.contains(FaultStatus::REFINLOW) {
        "REFIN- > 0.85 x Bias"
    } else if fault.contains(FaultStatus::REFINHIGH) {
        "REFIN- < 0.85 x Bias - FORCE- open"
    } else if fault.contains(FaultStatus::RTDINLOW) {
        "RTDIN- < 0.85 x Bias - FORCE- open"
    } else if fault.contains(FaultStatus::OVUV) {
        "Under/Over voltage"
    } else {
        "No fault"
    }
}

/// Nominal platinum RTD resistance in ohms at 0 °C.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RtdNominal(pub f32);

impl RtdNominal {
    /// 100 Ω platinum RTD.
    pub const PT100: Self = Self(100.0);
    /// 1000 Ω platinum RTD.
    pub const PT1000: Self = Self(1000.0);
}

/// Actual reference resistor value in ohms; use the value fitted to the board.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RefResistor(pub f32);

impl RefResistor {
    /// Common PT100 breakout reference resistor.
    pub const OHM_430: Self = Self(430.0);
    /// Common PT1000 breakout reference resistor.
    pub const OHM_4300: Self = Self(4300.0);
}

/// Wiring, filter and software calibration. Construction does not configure hardware.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    /// Lead configuration matching the board connections.
    pub wires: WireCount,
    /// Mains rejection filter.
    pub filter: FilterHz,
    /// RTD resistance at 0 °C, finite and positive.
    pub rtd_nominal: RtdNominal,
    /// Fitted reference resistance, finite and positive.
    pub ref_resistor: RefResistor,
}

impl Config {
    /// PT100, 430 Ω reference, three leads, 60 Hz rejection.
    pub const fn pt100_3wire() -> Self {
        Self {
            wires: WireCount::Three,
            filter: FilterHz::Hz60,
            rtd_nominal: RtdNominal::PT100,
            ref_resistor: RefResistor::OHM_430,
        }
    }

    /// PT1000, 4300 Ω reference, three leads, 60 Hz rejection.
    pub const fn pt1000_3wire() -> Self {
        Self {
            rtd_nominal: RtdNominal::PT1000,
            ref_resistor: RefResistor::OHM_4300,
            ..Self::pt100_3wire()
        }
    }

    fn valid(self) -> bool {
        positive_finite(self.rtd_nominal.0) && positive_finite(self.ref_resistor.0)
    }

    fn bits(self) -> u8 {
        (if self.wires == WireCount::Three {
            0x10
        } else {
            0
        }) | (if self.filter == FilterHz::Hz50 { 1 } else { 0 })
    }
}

/// Transport, configuration or measurement failure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Error<E> {
    /// Unmodified SPI device error, including chip-select errors from the adapter.
    Spi(E),
    /// Calibration resistance is nonpositive or nonfinite.
    InvalidConfig,
    /// Thresholds must be ordered 15-bit ADC counts (0 through 32767).
    InvalidThresholds,
    /// One-shot measurement was requested during continuous conversion.
    ContinuousMode,
    /// The sample fault bit was set; the associated latched status is preserved.
    /// An empty status still denotes a faulted sample.
    Fault(FaultStatus),
    /// Calibration and sample cannot produce a finite temperature.
    InvalidTemperature,
}

/// Async MAX31865 handle over an owned, borrowed or shared SPI device.
///
/// Configure the SPI controller for MSB-first mode 1 or 3, at most 5 MHz.
/// Use one logical handle per chip. Operations spanning transactions must not
/// race another handle. Bus recovery and cancellation cleanup belong to the caller.
pub struct Max31865<SPI> {
    spi: SPI,
    cfg: Config,
}

impl<SPI: SpiDevice<u8>> Max31865<SPI> {
    /// Validate calibration and create a handle without I/O. Call `init` next.
    pub fn new(spi: SPI, cfg: Config) -> Result<Self, Error<SPI::Error>> {
        if !cfg.valid() {
            return Err(Error::InvalidConfig);
        }
        Ok(Self { spi, cfg })
    }

    /// Return the device for reuse or recovery.
    pub fn into_inner(self) -> SPI {
        self.spi
    }

    /// Borrow the device for bus-specific recovery.
    pub fn device_mut(&mut self) -> &mut SPI {
        &mut self.spi
    }

    /// Last successfully applied configuration (or the constructor's request).
    /// This is not a hardware readback; external reset or a failed write can differ.
    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Apply wiring/filter, turn bias and continuous conversion off, clear faults,
    /// and set thresholds to the full 15-bit range. Does not wait for an in-flight
    /// conversion; after cancellation, allow 66 ms before starting a new one.
    pub async fn init(&mut self) -> Result<(), Error<SPI::Error>> {
        self.configure(self.cfg).await?;
        self.set_thresholds(0, 0x7fff).await
    }

    /// Apply new settings with bias/continuous mode off and faults cleared.
    /// Thresholds are preserved. Cache updates only after a successful write;
    /// after failure/cancellation, reapply settings before measuring.
    pub async fn configure(&mut self, cfg: Config) -> Result<(), Error<SPI::Error>> {
        if !cfg.valid() {
            return Err(Error::InvalidConfig);
        }
        self.write_u8(CONFIG, cfg.bits() | CLEAR_FAULT).await?;
        self.cfg = cfg;
        Ok(())
    }

    /// Enable/disable bias, preserving persistent configuration without replaying triggers.
    /// Wait at least 10.5 input RC time constants plus 1 ms before a conversion.
    pub async fn enable_bias(&mut self, on: bool) -> Result<(), Error<SPI::Error>> {
        self.update_config(BIAS, if on { BIAS } else { 0 }).await
    }

    /// Enable/disable continuous conversions. Enable and settle bias first.
    /// The first result needs 55 ms (60 Hz) or 66 ms (50 Hz). Subsequent samples
    /// may be read with `read_sample`; application scheduling/DRDY remains external.
    pub async fn auto_convert(&mut self, on: bool) -> Result<(), Error<SPI::Error>> {
        self.update_config(AUTO, if on { AUTO } else { 0 }).await
    }

    /// Clear latched faults without replaying one-shot/fault-cycle commands.
    pub async fn clear_fault(&mut self) -> Result<(), Error<SPI::Error>> {
        self.update_config(0, CLEAR_FAULT).await
    }

    /// Read latched fault status without starting a detection cycle or clearing it.
    pub async fn read_fault(&mut self) -> Result<FaultStatus, Error<SPI::Error>> {
        Ok(FaultStatus::from_bits_truncate(self.read_u8(FAULT).await?))
    }

    /// Issue a fault-cycle command, enabling bias and stopping continuous mode.
    ///
    /// Enable and settle bias first. Auto requires an RC time constant <=100 µs;
    /// wait at least 600 µs before reading status. For manual detection, wait
    /// at least 5 RC time constants after `ManualRun`, issue `ManualFinish`, then
    /// poll `fault_cycle_finished` with a delay and deadline before reading status.
    /// Faults remain latched until cleared.
    /// This method does not delay, report completion, or restore the previous mode.
    pub async fn start_fault_cycle(&mut self, cycle: FaultCycle) -> Result<(), Error<SPI::Error>> {
        let config = self.read_u8(CONFIG).await? & 0x11;
        let command = match cycle {
            FaultCycle::Auto => 0x04,
            FaultCycle::ManualRun => 0x08,
            FaultCycle::ManualFinish => 0x0c,
        };
        self.write_u8(CONFIG, config | BIAS | command).await
    }

    /// Whether fault-cycle control bits are idle. Manual cycle 1 remains busy
    /// until `ManualFinish` is issued. Performs one transaction, with no polling.
    pub async fn fault_cycle_finished(&mut self) -> Result<bool, Error<SPI::Error>> {
        Ok(self.read_u8(CONFIG).await? & 0x0c == 0)
    }

    /// Write ordered 15-bit ADC thresholds in one contiguous register transaction.
    /// Inputs use the same units as `read_rtd`, not the shifted register encoding.
    pub async fn set_thresholds(
        &mut self,
        lower: u16,
        upper: u16,
    ) -> Result<(), Error<SPI::Error>> {
        if lower > upper || upper > 0x7fff {
            return Err(Error::InvalidThresholds);
        }
        let [hi_msb, hi_lsb] = (upper << 1).to_be_bytes();
        let [lo_msb, lo_lsb] = (lower << 1).to_be_bytes();
        self.spi
            .write(&[HIGH_THRESHOLD | 0x80, hi_msb, hi_lsb, lo_msb, lo_lsb])
            .await
            .map_err(Error::Spi)
    }

    /// Read the low threshold as an unshifted 15-bit ADC count.
    pub async fn lower_threshold(&mut self) -> Result<u16, Error<SPI::Error>> {
        Ok(self.read_u16(LOW_THRESHOLD).await? >> 1)
    }

    /// Read the high threshold as an unshifted 15-bit ADC count.
    pub async fn upper_threshold(&mut self) -> Result<u16, Error<SPI::Error>> {
        Ok(self.read_u16(HIGH_THRESHOLD).await? >> 1)
    }

    /// Read the latest 15-bit result, rejecting faulted samples. Does not start
    /// or wait for a conversion; the caller must establish freshness first.
    pub async fn read_sample(&mut self) -> Result<u16, Error<SPI::Error>> {
        let raw = self.read_u16(RTD).await?;
        if raw & 1 != 0 {
            return Err(Error::Fault(self.read_fault().await?));
        }
        Ok(raw >> 1)
    }

    /// Take a fresh one-shot measurement using 11 ms bias settling and a 66 ms
    /// conversion wait (covers both filters). Use `read_rtd_with_settling` for
    /// an input RC network needing more than 11 ms. Rejects continuous mode.
    ///
    /// Bias is turned off after success and an attempt is made after any error
    /// once bias enable begins. The original measurement error wins over a cleanup
    /// error. Cancellation cannot run async cleanup: recover the bus, call `init`,
    /// and allow 66 ms before another conversion. Bus timeouts are caller-owned.
    pub async fn read_rtd<D: DelayNs>(&mut self, delay: &mut D) -> Result<u16, Error<SPI::Error>> {
        self.read_rtd_with_settling(delay, 11).await
    }

    /// One-shot measurement with a caller-selected bias settling time in ms,
    /// clamped to at least 11 ms. See `read_rtd` for errors and cancellation.
    pub async fn read_rtd_with_settling<D: DelayNs>(
        &mut self,
        delay: &mut D,
        settling_ms: u32,
    ) -> Result<u16, Error<SPI::Error>> {
        let config = self.read_u8(CONFIG).await?;
        if config & AUTO != 0 {
            return Err(Error::ContinuousMode);
        }
        let idle = config & 0x11;
        let result = async {
            self.write_u8(CONFIG, idle | BIAS | CLEAR_FAULT).await?;
            delay.delay_ms(settling_ms.max(11)).await;
            self.write_u8(CONFIG, idle | BIAS | ONE_SHOT).await?;
            delay.delay_ms(66).await;
            self.read_sample().await
        }
        .await;
        let cleanup = self.write_u8(CONFIG, idle).await;
        match result {
            Err(error) => Err(error),
            Ok(sample) => cleanup.map(|()| sample),
        }
    }

    /// Take a one-shot measurement and convert it to Celsius using stored calibration.
    /// Uses the default 11 ms bias settling time; for larger RC networks, combine
    /// `read_rtd_with_settling` with `calculate_temperature`.
    pub async fn read_temperature<D: DelayNs>(
        &mut self,
        delay: &mut D,
    ) -> Result<f32, Error<SPI::Error>> {
        let raw = self.read_rtd(delay).await?;
        calculate_temperature(raw, self.cfg.rtd_nominal.0, self.cfg.ref_resistor.0)
            .ok_or(Error::InvalidTemperature)
    }

    async fn update_config(&mut self, mask: u8, value: u8) -> Result<(), Error<SPI::Error>> {
        let current = self.read_u8(CONFIG).await?;
        self.write_u8(CONFIG, (current & PERSISTENT & !mask) | value)
            .await
    }

    async fn read_u8(&mut self, address: u8) -> Result<u8, Error<SPI::Error>> {
        let mut bytes = [0];
        self.read_n(address, &mut bytes).await?;
        Ok(bytes[0])
    }

    async fn read_u16(&mut self, address: u8) -> Result<u16, Error<SPI::Error>> {
        let mut bytes = [0; 2];
        self.read_n(address, &mut bytes).await?;
        Ok(u16::from_be_bytes(bytes))
    }

    async fn read_n(&mut self, address: u8, bytes: &mut [u8]) -> Result<(), Error<SPI::Error>> {
        use embedded_hal_async::spi::Operation;
        self.spi
            .transaction(&mut [Operation::Write(&[address]), Operation::Read(bytes)])
            .await
            .map_err(Error::Spi)
    }

    async fn write_u8(&mut self, address: u8, value: u8) -> Result<(), Error<SPI::Error>> {
        self.spi
            .write(&[address | 0x80, value])
            .await
            .map_err(Error::Spi)
    }
}

fn positive_finite(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

/// Convert an unshifted 15-bit ADC count to Celsius for an IEC 60751 platinum RTD.
/// Uses Adafruit's Callendar–Van Dusen quadratic and negative-temperature polynomial.
/// The approximation is intended for -200 through +850 °C; no range or accuracy
/// guarantee is made outside that interval. This function cannot detect wiring faults.
/// Returns `None` for invalid counts/calibration or a nonfinite calculation.
pub fn calculate_temperature(raw: u16, nominal: f32, reference: f32) -> Option<f32> {
    if raw > 0x7fff || !positive_finite(nominal) || !positive_finite(reference) {
        return None;
    }
    const A: f32 = 3.9083e-3;
    const B: f32 = -5.775e-7;
    let mut rt = raw as f32 / 32768.0 * reference;
    let discriminant = A * A - 4.0 * B + (4.0 * B / nominal) * rt;
    let temperature = (libm::sqrtf(discriminant) - A) / (2.0 * B);
    if !temperature.is_finite() {
        return None;
    }
    if temperature >= 0.0 {
        return Some(temperature);
    }
    rt = rt / nominal * 100.0;
    let mut rpoly = rt;
    let mut temperature = -242.02 + 2.2228 * rpoly;
    rpoly *= rt;
    temperature += 2.5859e-3 * rpoly;
    rpoly *= rt;
    temperature -= 4.8260e-6 * rpoly;
    rpoly *= rt;
    temperature -= 2.8183e-8 * rpoly;
    rpoly *= rt;
    temperature += 1.5243e-10 * rpoly;
    temperature.is_finite().then_some(temperature)
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
