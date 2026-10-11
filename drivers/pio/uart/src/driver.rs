use crate::{ConfigError, timing};
use core::convert::Infallible;
use embassy_rp::{
    Peri,
    clocks::clk_sys_freq,
    pio::{Common, Instance, PioPin, StateMachine},
    pio_programs::uart::{PioUartRx, PioUartRxProgram, PioUartTx, PioUartTxProgram},
};
use embedded_io_async::{ErrorType, Read, Write};

/// Load once per PIO block and share between lanes (14 instruction words).
pub struct Programs<'d, P: Instance> {
    tx: PioUartTxProgram<'d, P>,
    rx: PioUartRxProgram<'d, P>,
}

impl<'d, P: Instance> Programs<'d, P> {
    pub fn new(common: &mut Common<'d, P>) -> Self {
        Self {
            tx: PioUartTxProgram::new(common),
            rx: PioUartRxProgram::new(common),
        }
    }
}

/// Owned Embassy async TX/RX halves with embedded-io-async interfaces.
///
/// 8N1 only. Keep clk_sys unchanged for the entire lifetime. No DMA/background
/// writer or access to the owned halves is exposed, so flush can bound the tail.
/// Receive buffering and error reporting retain Embassy's limitations.
pub struct PioUart<'d, P: Instance, const TX: usize, const RX: usize> {
    tx: PioUartTx<'d, P, TX>,
    rx: PioUartRx<'d, P, RX>,
    flush_us: u64,
}

impl<'d, P: Instance, const TX: usize, const RX: usize> PioUart<'d, P, TX, RX> {
    /// BSP supplies the PIO, distinct state machines, pins, shared programs and
    /// baud. Bind Embassy's PIO interrupt handler before construction. Rejects
    /// rates that Embassy's integer divider would silently round.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        common: &mut Common<'d, P>,
        tx_sm: StateMachine<'d, P, TX>,
        rx_sm: StateMachine<'d, P, RX>,
        tx_pin: Peri<'d, impl PioPin>,
        rx_pin: Peri<'d, impl PioPin>,
        programs: &Programs<'d, P>,
        baud: u32,
    ) -> Result<Self, ConfigError> {
        let flush_us = timing::flush_us(clk_sys_freq(), baud)?;
        Ok(Self {
            tx: PioUartTx::new(baud, common, tx_sm, tx_pin, &programs.tx),
            rx: PioUartRx::new(baud, common, rx_sm, rx_pin, &programs.rx),
            flush_us,
        })
    }
}

impl<P: Instance, const TX: usize, const RX: usize> ErrorType for PioUart<'_, P, TX, RX> {
    type Error = Infallible;
}

impl<P: Instance, const TX: usize, const RX: usize> Write for PioUart<'_, P, TX, RX> {
    /// Delegates to Embassy. Cancellation may leave a prefix queued/in flight.
    async fn write(&mut self, bytes: &[u8]) -> Result<usize, Infallible> {
        self.tx.write(bytes).await
    }

    /// Wait for the maximum possible FIFO/OSR tail, including the last stop bit.
    /// Call again after cancellation; cancelling does not abort queued bytes.
    async fn flush(&mut self) -> Result<(), Infallible> {
        embassy_time::Timer::after_micros(self.flush_us).await;
        Ok(())
    }
}

impl<P: Instance, const TX: usize, const RX: usize> Read for PioUart<'_, P, TX, RX> {
    /// Return one byte without waiting to fill the buffer. Empty reads do not
    /// wait. Cancelling a pending read consumes no byte.
    async fn read(&mut self, bytes: &mut [u8]) -> Result<usize, Infallible> {
        if bytes.is_empty() {
            return Ok(0);
        }
        bytes[0] = self.rx.read_u8().await;
        Ok(1)
    }
}
