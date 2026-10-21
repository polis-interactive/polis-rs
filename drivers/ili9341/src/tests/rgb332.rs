use super::*;
use lcd_async::interface::InterfaceKind;

// Capture compressed pixel slices while exercising real controller init and
// window commands through the upstream SPI mock. The PIO tests independently
// execute expansion and WR edges from the assembled instructions.
struct Compressed {
    inner: SpiInterface<Bus, Pin>,
    slices: Rc<RefCell<Vec<(usize, usize)>>>,
}

impl Interface for Compressed {
    type Word = u8;
    type Error = SpiError<Fault, Fault>;
    const KIND: InterfaceKind = InterfaceKind::Parallel8Bit;

    async fn send_command(&mut self, command: u8, args: &[u8]) -> Result<(), Self::Error> {
        self.inner.send_command(command, args).await
    }

    async fn send_data_slice(&mut self, _: &[u8]) -> Result<(), Self::Error> {
        panic!("compressed pixels must use the explicit expansion operation")
    }
}

impl Rgb332Interface for Compressed {
    async fn send_rgb332(&mut self, pixels: &[u8]) -> Result<(), Self::Error> {
        self.slices
            .borrow_mut()
            .push((pixels.as_ptr() as usize, pixels.len()));
        self.inner.send_data_slice(pixels).await
    }
}

fn compressed(trace: &Shared) -> Compressed {
    Compressed {
        inner: interface(trace),
        slices: Rc::default(),
    }
}

fn display332<'a>(
    trace: &Shared,
    buffer: &'a mut [u8; RGB332_BUFFER_SIZE],
) -> Ili9341Rgb332<'a, Compressed, Pin> {
    block_on(Ili9341Rgb332::new(
        compressed(trace),
        Pin("reset", trace.clone()),
        buffer,
        Config::default(),
        &mut Delay(trace.clone()),
    ))
    .unwrap()
}

fn assert_frame332(writes: &[(bool, &[u8])], buffer: &[u8]) {
    assert_eq!(writes.len(), 7);
    assert_eq!(writes[0], (false, &[0x2a][..]));
    assert_eq!(writes[1], (true, &[0, 0, 1, 63][..]));
    assert_eq!(writes[2], (false, &[0x2b][..]));
    assert_eq!(writes[3], (true, &[0, 0, 0, 239][..]));
    assert_eq!(writes[4], (false, &[0x2c][..]));
    assert_eq!(writes[5], (true, &[][..]));
    assert_eq!(writes[6], (true, buffer));
}

#[test]
fn init_stays_rgb565_and_pixels_are_borrowed_rgb332() {
    for landscape in [Landscape::Normal, Landscape::Flipped] {
        let trace = Shared::default();
        let mut buffer = [0; RGB332_BUFFER_SIZE];
        for (i, byte) in buffer.iter_mut().enumerate() {
            *byte = i as u8;
        }
        let address = buffer.as_ptr() as usize;
        let interface = compressed(&trace);
        let slices = interface.slices.clone();
        let config = Config {
            landscape,
            ..Default::default()
        };
        let display = block_on(Ili9341Rgb332::new(
            interface,
            Pin("reset", trace.clone()),
            &mut buffer,
            config,
            &mut Delay(trace.clone()),
        ))
        .unwrap();
        assert_eq!(display.size(), Size::new(320, 240));
        let trace = trace.borrow();
        let writes = writes(&trace);
        assert_eq!(
            writes[1],
            (
                true,
                &[if landscape == Landscape::Normal {
                    0x68
                } else {
                    0xa8
                }][..]
            )
        );
        assert_eq!(writes[6], (false, &[0x3a][..]));
        assert_eq!(writes[7], (true, &[0x55][..]));
        assert_frame332(&writes[14..], display.buffer());
        assert_eq!(*slices.borrow(), [(address, RGB332_BUFFER_SIZE)]);
    }
}

#[test]
fn drawing_quantizes_and_clips_without_io_and_raw_edits_flush() {
    let trace = Shared::default();
    let mut buffer = [0; RGB332_BUFFER_SIZE];
    let mut display = display332(&trace, &mut buffer);
    trace.borrow_mut().events.clear();
    display
        .draw_iter([
            Pixel(Point::new(0, 0), Rgb565::RED),
            Pixel(Point::new(319, 0), Rgb565::GREEN),
            Pixel(Point::new(0, 1), Rgb565::BLUE),
            Pixel(Point::new(319, 239), Rgb565::WHITE),
            Pixel(Point::new(-1, 0), Rgb565::WHITE),
            Pixel(Point::new(320, 0), Rgb565::WHITE),
            Pixel(Point::new(0, 240), Rgb565::WHITE),
            Pixel(Point::new(i32::MAX, i32::MIN), Rgb565::WHITE),
        ])
        .unwrap();
    let mut expected = [0; RGB332_BUFFER_SIZE];
    expected[0] = 0xe0;
    expected[319] = 0x1c;
    expected[320] = 3;
    expected[RGB332_BUFFER_SIZE - 1] = 0xff;
    assert_eq!(display.buffer(), &expected);
    display.clear(Rgb565::RED).unwrap();
    assert!(display.buffer().iter().all(|b| *b == 0xe0));
    display.clear_buffer();
    display
        .fill_contiguous(
            &Rectangle::new(Point::new(-1, 0), Size::new(3, 1)),
            [Rgb565::RED, Rgb565::GREEN, Rgb565::BLUE],
        )
        .unwrap();
    assert_eq!(&display.buffer()[..3], &[0x1c, 3, 0]);
    display
        .fill_solid(
            &Rectangle::new(Point::new(319, 239), Size::new(2, 2)),
            Rgb565::WHITE,
        )
        .unwrap();
    assert_eq!(display.buffer()[RGB332_BUFFER_SIZE - 1], 0xff);
    assert!(trace.borrow().events.is_empty());
    for (i, byte) in display.buffer_mut().iter_mut().enumerate() {
        *byte = i as u8;
    }
    block_on(display.flush()).unwrap();
    assert_frame332(&writes(&trace.borrow()), display.buffer());
}

#[test]
fn quantization_covers_every_rgb565_color() {
    let trace = Shared::default();
    let mut buffer = [0; RGB332_BUFFER_SIZE];
    let mut display = display332(&trace, &mut buffer);
    display
        .draw_iter((0..=u16::MAX).map(|raw| {
            Pixel(
                Point::new(i32::from(raw) % 320, i32::from(raw) / 320),
                Rgb565::new((raw >> 11) as u8, ((raw >> 5) & 63) as u8, (raw & 31) as u8),
            )
        }))
        .unwrap();
    for raw in 0..=u16::MAX {
        assert_eq!(
            display.buffer()[raw as usize],
            (((raw & 0xe000) >> 8) | ((raw & 0x0700) >> 6) | ((raw & 0x0018) >> 3)) as u8
        );
    }
}

#[test]
fn failed_and_cancelled_flushes_retry_and_release_preserves_buffer() {
    let trace = Shared::default();
    let mut buffer = [0x42; RGB332_BUFFER_SIZE];
    let address = buffer.as_ptr();
    let mut display = display332(&trace, &mut buffer);
    for fail_at in 0..7 {
        *trace.borrow_mut() = Trace {
            fail_write: Some(fail_at),
            ..Default::default()
        };
        assert!(matches!(
            block_on(display.flush()),
            Err(SpiError::Spi(Fault))
        ));
        assert_eq!(trace.borrow().attempts, fail_at + 1);
        assert!(display.buffer().iter().all(|b| *b == 0x42));
        *trace.borrow_mut() = Trace::default();
        block_on(display.flush()).unwrap();
        assert_frame332(&writes(&trace.borrow()), display.buffer());
    }
    *trace.borrow_mut() = Trace::default();
    {
        let mut future = pin!(display.flush());
        let mut context = Context::from_waker(Waker::noop());
        for _ in 0..7 {
            assert!(future.as_mut().poll(&mut context).is_pending());
        }
    }
    assert_eq!(trace.borrow().attempts, 6); // Pixel write is pending.
    *trace.borrow_mut() = Trace::default();
    block_on(display.flush()).unwrap();
    assert_frame332(&writes(&trace.borrow()), display.buffer());
    let (_, reset, buffer) = display.release();
    assert_eq!(buffer.as_ptr(), address);
    assert_eq!(reset.0, "reset");
}

#[test]
fn initialization_failure_can_retry_with_borrowed_resources() {
    for fail_at in [0, 7, 13, 14, 20] {
        let trace = Shared::default();
        trace.borrow_mut().fail_write = Some(fail_at);
        let mut buffer = [0xa5; RGB332_BUFFER_SIZE];
        let mut interface = compressed(&trace);
        let mut reset = Pin("reset", trace.clone());
        let result = block_on(Ili9341Rgb332::new(
            &mut interface,
            &mut reset,
            &mut buffer,
            Config::default(),
            &mut Delay(trace.clone()),
        ));
        assert!(matches!(
            result,
            Err(InitError::Interface(SpiError::Spi(Fault)))
        ));
        drop(result);
        *trace.borrow_mut() = Trace::default();
        let display = block_on(Ili9341Rgb332::new(
            &mut interface,
            &mut reset,
            &mut buffer,
            Config::default(),
            &mut Delay(trace.clone()),
        ))
        .unwrap();
        assert!(display.buffer().iter().all(|b| *b == 0xa5));
        assert_frame332(&writes(&trace.borrow())[14..], display.buffer());
    }
}

#[test]
fn row_uploads_clip_and_use_one_window_with_the_original_pixel_slice() {
    let trace = Shared::default();
    let mut buffer = [0; RGB332_BUFFER_SIZE];
    for (i, pixel) in buffer.iter_mut().enumerate() {
        *pixel = i as u8;
    }
    let address = buffer.as_ptr() as usize;
    let interface = compressed(&trace);
    let slices = interface.slices.clone();
    let mut display = block_on(Ili9341Rgb332::new(
        interface,
        Pin("reset", trace.clone()),
        &mut buffer,
        Config::default(),
        &mut Delay(trace.clone()),
    ))
    .unwrap();
    for (start, end) in [(0, HEIGHT), (16, 32), (239, usize::MAX), (37, 38)] {
        *trace.borrow_mut() = Trace::default();
        block_on(display.flush_rows(start..end)).unwrap();
        let end = end.min(HEIGHT);
        let trace = trace.borrow();
        let writes = writes(&trace);
        assert_eq!(writes.len(), 7);
        assert_eq!(writes[0], (false, &[0x2a][..]));
        assert_eq!(writes[1], (true, &[0, 0, 1, 63][..]));
        assert_eq!(writes[2], (false, &[0x2b][..]));
        assert_eq!(writes[3], (true, &[0, start as u8, 0, (end - 1) as u8][..]));
        assert_eq!(writes[4], (false, &[0x2c][..]));
        assert_eq!(
            writes[6],
            (true, &display.buffer()[start * WIDTH..end * WIDTH])
        );
        assert_eq!(
            slices.borrow().last(),
            Some(&(address + start * WIDTH, (end - start) * WIDTH))
        );
    }
    for (start, end) in [(0, 0), (HEIGHT, HEIGHT + 16), (usize::MAX, 0), (16, 8)] {
        *trace.borrow_mut() = Trace::default();
        block_on(display.flush_rows(start..end)).unwrap();
        assert!(trace.borrow().events.is_empty());
    }
}
