use embassy_rp::{
    clocks::clk_sys_freq,
    dma,
    gpio::{Level, Output},
    pio::{
        Common, Config as PioConfig, Direction, FifoJoin, Instance, Irq, IrqFlags, LoadedProgram,
        Pin, ShiftConfig, ShiftDirection, StateMachine,
    },
};
use fixed::types::U24F8;

use crate::{Config, ConfigError, waveform};

mod pipeline;
pub use pipeline::{PioI8080Rgb332Pipeline, Rgb332Converter};

/// Owned display-bus pins. Create CS high before muxing the data and WR pins.
pub struct Pins<'d, P: Instance> {
    /// Consecutive GPIOs in D0..D7 order (D0 is the least significant bit).
    pub data: [Pin<'d, P>; 8],
    /// Active-low write strobe; may be any other PIO-capable GPIO.
    pub wr: Pin<'d, P>,
    /// High selects data; low selects a command.
    pub dc: Output<'d>,
    /// Active-low chip select. This bus is exclusively owned, not shared.
    pub cs: Output<'d>,
}

/// Async 8080 output using one SM, its matching IRQ flag, one DMA channel,
/// seven instruction slots, and eleven GPIOs. No allocator or time driver.
///
/// RD must stay high. Reset is controller-owned. Set pins/clock only through
/// this driver while it exists; do not clear its IRQ flag from another task.
/// Transfers finish after the last WR rising edge and hold time, not merely
/// DMA completion. DMA reads the original byte slice without a staging copy.
pub struct PioI8080<'d, P: Instance, const SM: usize> {
    sm: StateMachine<'d, P, SM>,
    dma: dma::Channel<'d>,
    irq: Irq<'d, P, SM>,
    flags: IrqFlags<'d, P>,
    pins: Pins<'d, P>,
    config: PioConfig<'d, P>,
    // Retain allocation of the instructions for the driver's lifetime.
    _program: LoadedProgram<'d, P>,
}

impl<'d, P: Instance, const SM: usize> PioI8080<'d, P, SM> {
    /// Configure an idle bus. The IRQ number must match the SM number.
    ///
    /// Construct `dma` with Embassy's bound DMA interrupt handler, and obtain
    /// `irq`/`flags` from `Pio`. Bind the PIO interrupt handler as well.
    /// Returns an error for invalid pins/timing. Consumes resources on error.
    /// Panics if the PIO block has fewer than seven free instruction slots.
    pub fn new(
        common: &mut Common<'d, P>,
        mut sm: StateMachine<'d, P, SM>,
        dma: dma::Channel<'d>,
        irq: Irq<'d, P, SM>,
        flags: IrqFlags<'d, P>,
        mut pins: Pins<'d, P>,
        timing: Config,
    ) -> Result<Self, ConfigError> {
        pins.cs.set_high();
        waveform::validate_pins(core::array::from_fn(|i| pins.data[i].pin()), pins.wr.pin())?;
        let divider = timing.divider(clk_sys_freq())?;
        sm.set_enable(false);
        sm.clear_fifos();
        sm.restart();
        let program = common.load_program(&waveform::program());
        let mut config = PioConfig::default();
        config.use_program(&program, &[&pins.wr]);
        let data = pins.data.each_ref();
        config.set_out_pins(&data);
        config.clock_divider = U24F8::from_num(divider);
        config.fifo_join = FifoJoin::TxOnly;
        config.shift_out = ShiftConfig {
            auto_fill: false,
            threshold: 32,
            direction: ShiftDirection::Right,
        };
        sm.set_config(&config);
        sm.set_pins(Level::Low, &data);
        sm.set_pins(Level::High, &[&pins.wr]);
        sm.set_pin_dirs(Direction::Out, &data);
        sm.set_pin_dirs(Direction::Out, &[&pins.wr]);
        pins.dc.set_high();
        flags.clear(SM);
        Ok(Self {
            sm,
            dma,
            irq,
            flags,
            pins,
            config,
            _program: program,
        })
    }

    /// Send a command byte followed by data parameters with CS held low.
    ///
    /// Dropping the future aborts DMA, stops the SM, discards queued bytes, and
    /// deselects the display before restoring WR high. A partial command may
    /// have reached the controller: reset/reinitialize it before relying on
    /// configuration after cancelled initialization. No timeout is imposed.
    pub async fn write_command(&mut self, command: u8, args: &[u8]) {
        let transaction = Transaction(self);
        transaction.0.pins.dc.set_low();
        transaction.0.pins.cs.set_low();
        transaction.0.write_bytes(&[command]).await;
        transaction.0.pins.dc.set_high();
        transaction.0.write_bytes(args).await;
    }

    /// Send data bytes, explicitly selecting data mode. Empty writes do nothing.
    ///
    /// Cancellation cleans up the bus as for `write_command`; already-written
    /// pixels are not rolled back. Reissue the address window before retrying.
    pub async fn write_data(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let transaction = Transaction(self);
        transaction.0.pins.dc.set_high();
        transaction.0.pins.cs.set_low();
        transaction.0.write_bytes(data).await;
    }

    async fn write_bytes(&mut self, bytes: &[u8]) {
        // Bounded count fits both RP2040 and RP2350 DMA counters.
        for chunk in bytes.chunks(65535) {
            self.flags.clear(SM);
            self.sm.tx().push((chunk.len() - 1) as u32);
            self.sm.set_enable(true);
            // Narrow writes replicate the byte across the FIFO word. The raw
            // program consumes its low byte; RGB332 uses the replicated bits.
            // Neither path packs or converts pixels on the CPU.
            self.sm.tx().dma_push(&mut self.dma, chunk, false).await;
            self.irq.wait().await;
        }
    }

    fn idle(&mut self) {
        self.sm.set_enable(false);
        // Deselect before forcing WR high: an aborted low pulse must not latch
        // another byte while selected. Panel content after cancellation is partial.
        self.pins.cs.set_high();
        self.sm.clear_fifos();
        self.sm.restart();
        self.sm.set_config(&self.config);
        self.sm.set_pins(Level::High, &[&self.pins.wr]);
        self.pins.dc.set_high();
        self.flags.clear(SM);
    }
}

// This guard outlives the awaited DMA transfer. Embassy's transfer Drop aborts
// DMA synchronously before this guard stops/reinitializes the state machine.
struct Transaction<'a, 'd, P: Instance, const SM: usize>(&'a mut PioI8080<'d, P, SM>);

impl<P: Instance, const SM: usize> Drop for Transaction<'_, '_, P, SM> {
    fn drop(&mut self) {
        self.0.idle();
    }
}

impl<P: Instance, const SM: usize> Drop for PioI8080<'_, P, SM> {
    fn drop(&mut self) {
        self.idle();
    }
}

/// An 8080 bus with optional RGB332-to-RGB565 expansion in PIO.
///
/// Uses the original SM, IRQ, DMA channel and pins, plus 24 instruction slots
/// (31 total including the raw-byte program). Commands and ordinary data remain
/// byte-exact. Pixel DMA reads one byte per pixel directly from caller memory.
pub struct PioI8080Rgb332<'d, P: Instance, const SM: usize> {
    bus: PioI8080<'d, P, SM>,
    config: PioConfig<'d, P>,
    program: LoadedProgram<'d, P>,
}

impl<'d, P: Instance, const SM: usize> PioI8080Rgb332<'d, P, SM> {
    /// Add RGB332 support to an idle bus on the same PIO block.
    ///
    /// Panics if 24 additional instruction slots are unavailable. The existing
    /// clock divider and bus timing are retained. No display commands are sent.
    pub fn new(bus: PioI8080<'d, P, SM>, common: &mut Common<'d, P>) -> Self {
        let program = common.load_program(&waveform::rgb332_program());
        let mut config = bus.config;
        config.use_program(&program, &[&bus.pins.wr]);
        config.shift_in = ShiftConfig {
            auto_fill: false,
            threshold: 32,
            direction: ShiftDirection::Left,
        };
        Self {
            bus,
            config,
            program,
        }
    }

    /// Send a byte-exact command and parameters with the original bus timing.
    pub async fn write_command(&mut self, command: u8, args: &[u8]) {
        self.bus.write_command(command, args).await;
    }

    /// Send ordinary byte-exact data, without pixel conversion.
    pub async fn write_data(&mut self, data: &[u8]) {
        self.bus.write_data(data).await;
    }

    /// Expand packed `RRRGGGBB` pixels to big-endian RGB565 using PIO + DMA.
    ///
    /// Channels use bit replication: `abc -> abcab`, `abc -> abcabc`, and
    /// `ab -> ababa`. Black and white map to 0x0000 and 0xffff. The panel must
    /// already have a suitable window and RGB565 memory-write mode selected.
    /// Each pixel takes 25 PIO cycles plus FIFO stalls, emitting two WR pulses.
    /// The original byte-mode configuration is restored after completion or
    /// cancellation. Cancellation has the same cleanup contract as `write_data`.
    pub async fn write_rgb332(&mut self, pixels: &[u8]) {
        if pixels.is_empty() {
            return;
        }
        let transaction = Transaction(&mut self.bus);
        transaction.0.sm.set_config(&self.config);
        transaction.0.pins.dc.set_high();
        transaction.0.pins.cs.set_low();
        // The transfer header counts input elements for either program.
        transaction.0.write_bytes(pixels).await;
    }

    /// Release the idle raw-byte bus and free the RGB332 instruction slots.
    ///
    /// `common` must belong to the same PIO instance. Ordinary Drop stops the
    /// bus but does not reclaim Embassy's instruction allocation; use this
    /// method when the block will be reused without reconstructing its owner.
    pub fn release(mut self, common: &mut Common<'d, P>) -> PioI8080<'d, P, SM> {
        self.bus.idle();
        // SAFETY: the only SM that can use this private program is disabled
        // and has been reset to the distinct raw-byte program. No program or
        // config handle was exposed to another SM.
        unsafe { common.free_instr(self.program.used_memory) };
        self.bus
    }
}
