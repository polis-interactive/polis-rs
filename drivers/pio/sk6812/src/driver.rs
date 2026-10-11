// Adapted from embassy-rp 0.10.0, src/pio_programs/ws2812.rs.
// Copyright (c) Embassy project contributors
// Portions copyright (c) rp-rs organization
// SPDX-License-Identifier: MIT

use embassy_rp::clocks::clk_sys_freq;
use embassy_rp::gpio::{Level, Pull};
use embassy_rp::pio::{
    Common, Config, Direction, FifoJoin, Instance, LoadedProgram, PioPin, ShiftConfig,
    ShiftDirection, StateMachine,
};
use embassy_rp::{Peri, dma, interrupt};
use embassy_time::Timer;
use fixed::types::U24F8;

use crate::{RGBW, waveform};

/// Four shared PIO instructions; load once per PIO block for all LED chains.
pub struct PioSk6812Program<'d, P: Instance> {
    program: LoadedProgram<'d, P>,
}

impl<'d, P: Instance> PioSk6812Program<'d, P> {
    /// Load the waveform. Panics if the block has insufficient instruction space.
    pub fn new(common: &mut Common<'d, P>) -> Self {
        Self {
            program: common.load_program(&waveform::program()),
        }
    }
}

/// One GRBW chain, using one state machine, one DMA channel, and one output pin.
///
/// Board support supplies all resources. Keep `clk_sys` unchanged while this
/// driver exists, and provide enough DMA bandwidth to avoid mid-frame FIFO
/// underflow.
pub struct PioSk6812<'d, P: Instance, const SM: usize, const N: usize> {
    dma: dma::Channel<'d>,
    sm: StateMachine<'d, P, SM>,
    drain_and_reset_us: u64,
    ready: bool,
}

impl<'d, P: Instance, const SM: usize, const N: usize> PioSk6812<'d, P, SM, N> {
    /// Configure an initially low output. The first write includes a reset.
    ///
    /// Requires an Embassy time driver supplied by the application.
    /// Panics if `N` is zero or `clk_sys` is below 32 MHz.
    pub fn new<D: dma::ChannelInstance>(
        common: &mut Common<'d, P>,
        mut sm: StateMachine<'d, P, SM>,
        dma: Peri<'d, D>,
        irq: impl interrupt::typelevel::Binding<D::Interrupt, dma::InterruptHandler<D>> + 'd,
        pin: Peri<'d, impl PioPin>,
        program: &PioSk6812Program<'d, P>,
    ) -> Self
    where
        P: 'static,
    {
        assert!(N > 0, "SK6812 chain must contain at least one pixel");
        let timing = waveform::timing(clk_sys_freq());
        sm.set_enable(false);
        sm.clear_fifos();
        sm.restart();

        // Preload this owned output before make_pio_pin disables reset pulls.
        // Otherwise the translator input could float during mux handover.
        // PIO SET affects only this pin.
        // Use public PAC registers for the selected owned PIO instance.
        // RP2350A also exposes PIO2; its GPIO indices remain below 32.
        use embassy_rp::pac::io::vals::Gpio0ctrlFuncsel;
        let (pio, function) = if core::any::TypeId::of::<P>()
            == core::any::TypeId::of::<embassy_rp::peripherals::PIO0>()
        {
            (embassy_rp::pac::PIO0, Gpio0ctrlFuncsel::PIO0_0)
        } else if core::any::TypeId::of::<P>()
            == core::any::TypeId::of::<embassy_rp::peripherals::PIO1>()
        {
            (embassy_rp::pac::PIO1, Gpio0ctrlFuncsel::PIO1_0)
        } else {
            #[cfg(feature = "rp235xa")]
            {
                assert_eq!(
                    core::any::TypeId::of::<P>(),
                    core::any::TypeId::of::<embassy_rp::peripherals::PIO2>()
                );
                (embassy_rp::pac::PIO2, Gpio0ctrlFuncsel::PIO2_0)
            }
            #[cfg(not(feature = "rp235xa"))]
            unreachable!("unsupported PIO instance")
        };
        let regs = pio.sm(SM);
        let saved_pins = regs.pinctrl().read();
        let saved_exec = regs.execctrl().read();
        regs.pinctrl().write(|w| {
            w.set_set_base(pin.pin());
            w.set_set_count(1);
        });
        regs.execctrl().modify(|w| w.set_out_sticky(false));
        // SAFETY: this owned SM is disabled, sideset is disabled by pinctrl,
        // and SET targets only the owned data pin. No program is executing.
        unsafe {
            sm.exec_instr(0xe000); // SET PINS, 0
            sm.exec_instr(0xe081); // SET PINDIRS, 1
        }
        regs.pinctrl().write_value(saved_pins);
        regs.execctrl().write_value(saved_exec);
        // Select the already-low output while pulls are still intact. HAL's
        // make_pio_pin clears pulls before its own FUNCSEL write, so simply
        // preloading the SM is insufficient to bridge that earlier interval.
        embassy_rp::pac::IO_BANK0
            .gpio(pin.pin() as usize)
            .ctrl()
            .write(|w| w.set_funcsel(function as _));
        let mut pin = common.make_pio_pin(pin);
        // Keep the translator input defined if the output is later disabled.
        pin.set_pull(Pull::Down);
        sm.set_pins(Level::Low, &[&pin]);
        sm.set_pin_dirs(Direction::Out, &[&pin]);

        let mut config = Config::default();
        config.use_program(&program.program, &[&pin]);
        config.clock_divider = U24F8::from_bits(timing.divider_256);
        config.fifo_join = FifoJoin::TxOnly;
        config.shift_out = ShiftConfig {
            auto_fill: true,
            threshold: 32,
            direction: ShiftDirection::Left,
        };
        sm.set_config(&config);
        sm.set_enable(true);

        Self {
            dma: dma::Channel::new(dma, irq),
            sm,
            drain_and_reset_us: timing.drain_and_reset_us,
            ready: false,
        }
    }

    /// Send exactly `N` pixels, most-significant bit first in GRBW order.
    ///
    /// Returns after the FIFO/shift register have drained and at least 250 us
    /// of reset-low time has elapsed. Uses an inline `4 * N` byte DMA buffer
    /// in the future; no allocation. Chains may be written concurrently.
    ///
    /// Cancellation can display a partial frame. Embassy aborts the DMA when
    /// its future is dropped; the next write waits for the remaining data to
    /// drain and reset before starting a complete frame.
    pub async fn write(&mut self, colors: &[RGBW<u8>; N]) {
        if !self.ready {
            Timer::after_micros(self.drain_and_reset_us).await;
        }
        let words = colors.map(waveform::pack);
        self.ready = false;
        self.sm.tx().dma_push(&mut self.dma, &words, false).await;
        Timer::after_micros(self.drain_and_reset_us).await;
        self.ready = true;
    }
}

impl<P: Instance, const SM: usize, const N: usize> led_output::LedOutput
    for PioSk6812<'_, P, SM, N>
{
    const PIXEL_COUNT: usize = N;

    async fn write(&mut self, pixels: &[RGBW<u8>]) -> Result<(), led_output::FrameLengthError> {
        let colors = pixels
            .try_into()
            .map_err(|_| led_output::FrameLengthError {
                expected: N,
                actual: pixels.len(),
            })?;
        self.write(colors).await;
        Ok(())
    }
}
