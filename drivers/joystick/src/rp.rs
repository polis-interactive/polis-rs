//! Embassy RP ADC adapter. The consuming BSP owns pins, interrupts and the ADC.
use crate::AxisSource;
use embassy_rp::adc::{Adc, Async, Channel, Error};

/// Borrow an async on-chip ADC and two already-configured analog pin channels.
pub struct RpAxes<'a, 'd> {
    /// Shared on-chip ADC.
    pub adc: &'a mut Adc<'d, Async>,
    /// X analog pin.
    pub x: &'a mut Channel<'d>,
    /// Y analog pin.
    pub y: &'a mut Channel<'d>,
}
impl AxisSource for RpAxes<'_, '_> {
    type Error = Error;
    async fn read_axes(&mut self) -> Result<[i16; 2], Error> {
        let x = self.adc.read(self.x).await?;
        let y = self.adc.read(self.y).await?;
        Ok([x as i16, y as i16])
    }
}
