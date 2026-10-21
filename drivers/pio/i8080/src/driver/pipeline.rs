use super::*;
use embassy_futures::join::join;

/// Pinless RGB332-to-RGB565 converter using one SM and two DMA channels.
///
/// Allocates 14 instruction slots and runs at the system clock (divider 1).
/// May share the output bus's PIO block or use another block. No PIO IRQ is
/// needed: RX DMA completion proves that every input pixel has been converted.
pub struct Rgb332Converter<'d, P: Instance, const SM: usize> {
    sm: StateMachine<'d, P, SM>,
    input: dma::Channel<'d>,
    output: dma::Channel<'d>,
    config: PioConfig<'d, P>,
    _program: LoadedProgram<'d, P>,
}

impl<'d, P: Instance, const SM: usize> Rgb332Converter<'d, P, SM> {
    /// Reserve a converter. Bind DMA interrupts for both supplied channels.
    ///
    /// Panics if the block has fewer than 14 free instruction slots. As with
    /// the raw bus, ordinary Drop does not reclaim the instruction allocation.
    pub fn new(
        common: &mut Common<'d, P>,
        mut sm: StateMachine<'d, P, SM>,
        input: dma::Channel<'d>,
        output: dma::Channel<'d>,
    ) -> Self {
        sm.set_enable(false);
        sm.clear_fifos();
        sm.restart();
        let program = common.load_program(&waveform::rgb332_converter_program());
        let mut config = PioConfig::default();
        config.use_program(&program, &[]);
        config.clock_divider = U24F8::from_num(1);
        config.shift_out = ShiftConfig {
            auto_fill: false,
            threshold: 32,
            direction: ShiftDirection::Right,
        };
        config.shift_in = ShiftConfig {
            auto_fill: false,
            threshold: 32,
            direction: ShiftDirection::Left,
        };
        sm.set_config(&config);
        Self {
            sm,
            input,
            output,
            config,
            _program: program,
        }
    }

    async fn convert(&mut self, pixels: &[u8], words: &mut [u16]) {
        debug_assert_eq!(pixels.len(), words.len());
        debug_assert!(!pixels.is_empty() && pixels.len() <= 32767);
        let transaction = Conversion(self);
        transaction.0.sm.set_enable(true);
        let (rx, tx) = transaction.0.sm.rx_tx();
        // Each channel uses its own FIFO's DREQ. RX uses halfword byte-swap
        // so little-endian RAM contains the LCD's high byte followed by low.
        // Both transfers must finish before a staging buffer can be published.
        join(
            rx.dma_pull(&mut transaction.0.output, words, true),
            tx.dma_push(&mut transaction.0.input, pixels, false),
        )
        .await;
    }

    fn idle(&mut self) {
        self.sm.set_enable(false);
        self.sm.clear_fifos();
        self.sm.restart();
        self.sm.set_config(&self.config);
    }
}

struct Conversion<'a, 'd, P: Instance, const SM: usize>(&'a mut Rgb332Converter<'d, P, SM>);

impl<P: Instance, const SM: usize> Drop for Conversion<'_, '_, P, SM> {
    fn drop(&mut self) {
        // The joined DMA futures are dropped/aborted before this guard.
        self.0.idle();
    }
}

impl<P: Instance, const SM: usize> Drop for Rgb332Converter<'_, P, SM> {
    fn drop(&mut self) {
        self.idle();
    }
}

/// Two-SM RGB332 pipeline with caller-owned, alternating RGB565 buffers.
///
/// One DMA feeds the converter, another drains it into RAM, and the raw bus's
/// DMA sends the preceding buffer to the LCD concurrently. The CPU schedules
/// chunks but never converts or copies pixels. Uses 21 PIO instructions total,
/// two SMs, three DMA channels, and only the output SM's IRQ. Both SMs can be
/// in one PIO block. Buffers add SRAM traffic compared with the one-SM path.
pub struct PioI8080Rgb332Pipeline<
    'b,
    'd,
    P: Instance,
    const SM: usize,
    C: Instance,
    const CONVERT: usize,
> {
    bus: PioI8080<'d, P, SM>,
    converter: Rgb332Converter<'d, C, CONVERT>,
    scratch: &'b mut [u16],
}

impl<'b, 'd, P: Instance, const SM: usize, C: Instance, const CONVERT: usize>
    PioI8080Rgb332Pipeline<'b, 'd, P, SM, C, CONVERT>
{
    /// Combine an idle raw bus, converter and scratch storage.
    ///
    /// Splits `scratch` into two equal buffers; an odd final element is unused.
    /// Each buffer holds at most 32,767 pixels. For two 320×8 strips supply
    /// 5,120 `u16` elements (10 KiB). Panics if fewer than two elements are
    /// supplied. No display commands are sent and no memory is allocated.
    pub fn new(
        bus: PioI8080<'d, P, SM>,
        converter: Rgb332Converter<'d, C, CONVERT>,
        scratch: &'b mut [u16],
    ) -> Self {
        assert!(
            scratch.len() >= 2,
            "two staging buffers need at least two words"
        );
        Self {
            bus,
            converter,
            scratch,
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

    /// Expand RGB332 pixels while transmitting the preceding converted chunk.
    ///
    /// CS stays asserted across chunks. Completion includes the final WR hold
    /// time. Cancelling aborts each stage's DMA before resetting its SM, and
    /// deselects the LCD. Already-written pixels remain; reissue the
    /// address window before retrying. Empty slices do nothing.
    pub async fn write_rgb332(&mut self, pixels: &[u8]) {
        if pixels.is_empty() {
            return;
        }
        let size = (self.scratch.len() / 2).min(32767);
        let (mut current, mut next) = self.scratch[..2 * size].split_at_mut(size);
        let mut chunks = pixels.chunks(size);
        let first = chunks.next().unwrap();
        self.converter
            .convert(first, &mut current[..first.len()])
            .await;
        let transaction = Transaction(&mut self.bus);
        transaction.0.pins.dc.set_high();
        transaction.0.pins.cs.set_low();
        let mut count = first.len();
        for chunk in chunks {
            join(
                transaction.0.write_bytes(wire_bytes(&current[..count])),
                self.converter.convert(chunk, &mut next[..chunk.len()]),
            )
            .await;
            core::mem::swap(&mut current, &mut next);
            count = chunk.len();
        }
        transaction
            .0
            .write_bytes(wire_bytes(&current[..count]))
            .await;
    }
}

fn wire_bytes(words: &[u16]) -> &[u8] {
    // SAFETY: u16 storage is initialized and contiguous. All u8 patterns are
    // valid; the shared view has the same lifetime and cannot mutate storage.
    // RX DMA already put the bytes in wire order; there is no CPU byte-swap.
    unsafe { core::slice::from_raw_parts(words.as_ptr().cast(), core::mem::size_of_val(words)) }
}
