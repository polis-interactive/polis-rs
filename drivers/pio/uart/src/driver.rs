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

/// Sticky hardware receive faults since the previous acknowledgement.
/// A set flag means at least one event, not an exact count of lost bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RxFaults {
    /// The RX state machine stalled trying to push into its full FIFO.
    pub fifo_stalled: bool,
    /// A character had an invalid stop bit, or the line signalled a break.
    pub framing: bool,
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
    registers: embassy_rp::pac::pio::Pio,
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
        // Embassy keeps the instance registers private. Identify the block from
        // the FIFO pointer of the state machine we own, without stealing a token.
        let registers = if rx_sm.rx_fifo_ptr() == embassy_rp::pac::PIO0.rxf(RX).as_ptr() {
            embassy_rp::pac::PIO0
        } else if rx_sm.rx_fifo_ptr() == embassy_rp::pac::PIO1.rxf(RX).as_ptr() {
            embassy_rp::pac::PIO1
        } else {
            #[cfg(feature = "rp235xa")]
            {
                embassy_rp::pac::PIO2
            }
            #[cfg(not(feature = "rp235xa"))]
            {
                unreachable!("unknown PIO instance")
            }
        };
        Ok(Self {
            tx: PioUartTx::new(baud, common, tx_sm, tx_pin, &programs.tx),
            rx: PioUartRx::new(baud, common, rx_sm, rx_pin, &programs.rx),
            flush_us,
            registers,
        })
    }
}

impl<P: Instance, const TX: usize, const RX: usize> PioUart<'_, P, TX, RX> {
    /// Read and acknowledge only this receiver's sticky FIFO-stall/framing flags.
    /// Does not read FIFO data, alter direction, reset the SM, or discard bytes.
    /// Call from the bus owner outside time-critical receive windows. Events may
    /// coalesce between calls; an overflow can corrupt reception before this is
    /// observed. `Read` retains its existing `Infallible` contract.
    pub fn take_rx_faults(&self) -> RxFaults {
        let pio = self.registers;
        let faults = RxFaults {
            fifo_stalled: pio.fdebug().read().rxstall() & (1 << RX) != 0,
            framing: pio.irq().read().0 & (1 << (RX + 4)) != 0,
        };
        // Write-one-to-clear only the owned SM's flags; leave all other lanes alone.
        if faults.fifo_stalled {
            pio.fdebug().write(|w| w.set_rxstall(1 << RX));
        }
        if faults.framing {
            pio.irq().write(|w| w.0 = 1 << (RX + 4));
        }
        faults
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
