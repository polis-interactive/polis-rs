extern crate std;

mod rgb332;

use super::*;
use core::{
    future::{Future, poll_fn},
    pin::pin,
    task::{Context, Poll, Waker},
};
use embassy_futures::block_on;
use embedded_graphics_core::{geometry::Point, pixelcolor::RgbColor};
use embedded_hal::{digital, spi};
use embedded_hal_async::spi::SpiDevice;
use std::{cell::RefCell, rc::Rc, vec::Vec};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Fault;
impl spi::Error for Fault {
    fn kind(&self) -> spi::ErrorKind {
        spi::ErrorKind::Other
    }
}
impl digital::Error for Fault {
    fn kind(&self) -> digital::ErrorKind {
        digital::ErrorKind::Other
    }
}

#[derive(Debug, PartialEq)]
enum Event {
    Pin(&'static str, bool),
    Delay(u32),
    Write(bool, Vec<u8>),
}

#[derive(Default)]
struct Trace {
    events: Vec<Event>,
    dc: bool,
    attempts: usize,
    fail_write: Option<usize>,
    fail_pin: Option<(&'static str, bool)>,
}
type Shared = Rc<RefCell<Trace>>;

async fn suspend() {
    let mut pending = true;
    poll_fn(|cx| {
        if core::mem::take(&mut pending) {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;
}

struct Bus(Shared);
impl spi::ErrorType for Bus {
    type Error = Fault;
}
impl SpiDevice for Bus {
    async fn transaction(
        &mut self,
        operations: &mut [spi::Operation<'_, u8>],
    ) -> Result<(), Fault> {
        suspend().await;
        let [spi::Operation::Write(bytes)] = operations else {
            panic!("expected one write")
        };
        let mut trace = self.0.borrow_mut();
        let dc = trace.dc;
        trace.events.push(Event::Write(dc, bytes.to_vec()));
        let index = trace.attempts;
        trace.attempts += 1;
        if trace.fail_write == Some(index) {
            Err(Fault)
        } else {
            Ok(())
        }
    }
}

struct Pin(&'static str, Shared);
impl digital::ErrorType for Pin {
    type Error = Fault;
}
impl OutputPin for Pin {
    fn set_low(&mut self) -> Result<(), Fault> {
        self.set(false)
    }
    fn set_high(&mut self) -> Result<(), Fault> {
        self.set(true)
    }
}
impl Pin {
    fn set(&mut self, high: bool) -> Result<(), Fault> {
        let mut trace = self.1.borrow_mut();
        trace.events.push(Event::Pin(self.0, high));
        if trace.fail_pin == Some((self.0, high)) {
            return Err(Fault);
        }
        if self.0 == "dc" {
            trace.dc = high;
        }
        Ok(())
    }
}

struct Delay(Shared);
impl DelayNs for Delay {
    async fn delay_ns(&mut self, ns: u32) {
        suspend().await;
        self.0.borrow_mut().events.push(Event::Delay(ns));
    }
}

fn interface(trace: &Shared) -> SpiInterface<Bus, Pin> {
    SpiInterface::new(Bus(trace.clone()), Pin("dc", trace.clone()))
}

fn display<'a>(
    trace: &Shared,
    buffer: &'a mut [u8; BUFFER_SIZE],
    config: Config,
) -> Ili9341Async<'a, SpiInterface<Bus, Pin>, Pin> {
    block_on(Ili9341Async::new(
        interface(trace),
        Pin("reset", trace.clone()),
        buffer,
        config,
        &mut Delay(trace.clone()),
    ))
    .unwrap()
}

fn writes(trace: &Trace) -> Vec<(bool, &[u8])> {
    trace
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Write(dc, data) => Some((*dc, data.as_slice())),
            _ => None,
        })
        .collect()
}

fn assert_frame(writes: &[(bool, &[u8])], buffer: &[u8]) {
    assert_frame_size(writes, buffer, 320, 240);
}

fn assert_frame_size(writes: &[(bool, &[u8])], buffer: &[u8], width: usize, height: usize) {
    assert_eq!(writes.len(), height / 8 * 7);
    let row_bytes = width * 2;
    let x_end = ((width - 1) as u16).to_be_bytes();
    for (strip, group) in writes.as_chunks::<7>().0.iter().enumerate() {
        let y_start = ((strip * 8) as u16).to_be_bytes();
        let y_end = ((strip * 8 + 7) as u16).to_be_bytes();
        assert_eq!(group[0], (false, &[0x2A][..]));
        assert_eq!(group[1], (true, &[0, 0, x_end[0], x_end[1]][..]));
        assert_eq!(group[2], (false, &[0x2B][..]));
        assert_eq!(
            group[3],
            (true, &[y_start[0], y_start[1], y_end[0], y_end[1]][..])
        );
        assert_eq!(group[4], (false, &[0x2C][..]));
        assert_eq!(group[5], (true, &[][..]));
        assert_eq!(
            group[6],
            (
                true,
                &buffer[strip * 8 * row_bytes..(strip + 1) * 8 * row_bytes]
            )
        );
    }
}

#[test]
fn quarter_turns_preserve_mirroring_and_use_portrait_strides_and_windows() {
    for (landscape, mirror_horizontal, clockwise, counterclockwise) in [
        (Landscape::Normal, false, 0xc8, 0x08),
        (Landscape::Flipped, false, 0x08, 0xc8),
        (Landscape::Normal, true, 0x48, 0x88),
        (Landscape::Flipped, true, 0x88, 0x48),
    ] {
        for (rotation, madctl) in [
            (Rotation::Deg90, clockwise),
            (Rotation::Deg270, counterclockwise),
        ] {
            let trace = Shared::default();
            let mut buffer = [0; BUFFER_SIZE];
            let mut display = display(
                &trace,
                &mut buffer,
                Config {
                    landscape,
                    mirror_horizontal,
                    rotation,
                    ..Default::default()
                },
            );
            assert_eq!(display.size(), Size::new(240, 320));
            {
                let trace = trace.borrow();
                let writes = writes(&trace);
                assert_eq!(writes[1], (true, &[madctl][..]));
                assert_frame_size(&writes[14..], display.buffer(), 240, 320);
            }
            trace.borrow_mut().events.clear();
            display
                .draw_iter([
                    Pixel(Point::new(239, 0), Rgb565::GREEN),
                    Pixel(Point::new(0, 1), Rgb565::BLUE),
                    Pixel(Point::new(239, 319), Rgb565::WHITE),
                    Pixel(Point::new(240, 0), Rgb565::RED),
                    Pixel(Point::new(0, 320), Rgb565::RED),
                ])
                .unwrap();
            let mut expected = [0; BUFFER_SIZE];
            expected[478..480].copy_from_slice(&[0x07, 0xe0]);
            expected[480..482].copy_from_slice(&[0x00, 0x1f]);
            expected[BUFFER_SIZE - 2..].copy_from_slice(&[0xff, 0xff]);
            assert_eq!(display.buffer(), &expected);
            assert!(trace.borrow().events.is_empty());
            block_on(display.flush()).unwrap();
            assert_frame_size(&writes(&trace.borrow()), display.buffer(), 240, 320);
        }
    }
}

#[test]
fn reset_timing_rgb565_landscape_and_initial_frame() {
    for (config, madctl, inversion) in [
        (Config::default(), 0x68, 0x20),
        (
            Config {
                landscape: Landscape::Flipped,
                color_order: ColorOrder::Rgb,
                inversion: ColorInversion::Inverted,
                ..Default::default()
            },
            0xA0,
            0x21,
        ),
        (
            Config {
                mirror_horizontal: true,
                ..Default::default()
            },
            0xE8,
            0x20,
        ),
        (
            Config {
                landscape: Landscape::Flipped,
                mirror_horizontal: true,
                ..Default::default()
            },
            0x28,
            0x20,
        ),
    ] {
        let trace = Shared::default();
        let mut buffer = [0x5A; BUFFER_SIZE];
        let display = display(&trace, &mut buffer, config);
        assert_eq!(display.size(), Size::new(320, 240));
        let trace = trace.borrow();
        assert_eq!(
            &trace.events[..4],
            &[
                Event::Pin("reset", false),
                Event::Delay(10_000),
                Event::Pin("reset", true),
                Event::Delay(5_000_000),
            ]
        );
        let delays: Vec<_> = trace
            .events
            .iter()
            .filter_map(|e| {
                if let Event::Delay(ns) = e {
                    Some(*ns)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(delays, [10_000, 5_000_000, 120_000_000, 140_000_000]);
        let writes = writes(&trace);
        assert_eq!(
            &writes[..14],
            &[
                (false, &[0x36][..]),
                (true, &[madctl][..]),
                (false, &[0xB4][..]),
                (true, &[0][..]),
                (false, &[inversion][..]),
                (true, &[][..]),
                (false, &[0x3A][..]),
                (true, &[0x55][..]),
                (false, &[0x13][..]),
                (true, &[][..]),
                (false, &[0x11][..]),
                (true, &[][..]),
                (false, &[0x29][..]),
                (true, &[][..]),
            ]
        );
        assert_frame(&writes[14..], display.buffer());
    }
}

#[test]
fn rgb_byte_order_clipping_and_full_frame_strides() {
    let trace = Shared::default();
    let mut buffer = [0; BUFFER_SIZE];
    let mut display = display(&trace, &mut buffer, Config::default());
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
    let mut expected = [0; BUFFER_SIZE];
    expected[..2].copy_from_slice(&[0xF8, 0]);
    expected[638..640].copy_from_slice(&[0x07, 0xE0]);
    expected[640..642].copy_from_slice(&[0, 0x1F]);
    expected[BUFFER_SIZE - 2..].fill(0xFF);
    assert_eq!(display.buffer(), &expected);
    assert!(trace.borrow().events.is_empty());
    block_on(display.flush()).unwrap();
    assert_frame(&writes(&trace.borrow()), &expected);
    trace.borrow_mut().events.clear();
    block_on(display.flush()).unwrap();
    assert_frame(&writes(&trace.borrow()), &expected);
}

#[test]
fn clears_and_clipped_contiguous_colors_stay_local() {
    let trace = Shared::default();
    let mut buffer = [0; BUFFER_SIZE];
    let mut display = display(&trace, &mut buffer, Config::default());
    trace.borrow_mut().events.clear();
    display.clear(Rgb565::RED).unwrap();
    assert!(
        display
            .buffer()
            .as_chunks::<2>()
            .0
            .iter()
            .all(|p| *p == [0xF8, 0])
    );
    display.clear_buffer();
    assert!(display.buffer().iter().all(|b| *b == 0));
    display
        .fill_contiguous(
            &Rectangle::new(Point::new(-1, 0), Size::new(3, 1)),
            [Rgb565::RED, Rgb565::GREEN, Rgb565::BLUE],
        )
        .unwrap();
    assert_eq!(&display.buffer()[..4], &[0x07, 0xE0, 0, 0x1F]);
    display
        .fill_solid(
            &Rectangle::new(Point::new(319, 239), Size::new(2, 2)),
            Rgb565::WHITE,
        )
        .unwrap();
    assert_eq!(&display.buffer()[BUFFER_SIZE - 2..], &[0xFF, 0xFF]);
    assert!(trace.borrow().events.is_empty());
}

#[test]
fn every_flush_spi_failure_preserves_pixels_and_can_retry() {
    let trace = Shared::default();
    let mut buffer = [0xA5; BUFFER_SIZE];
    let mut display = display(&trace, &mut buffer, Config::default());
    for fail_at in 0..210 {
        *trace.borrow_mut() = Trace {
            fail_write: Some(fail_at),
            ..Default::default()
        };
        assert!(matches!(
            block_on(display.flush()),
            Err(SpiError::Spi(Fault))
        ));
        assert_eq!(trace.borrow().attempts, fail_at + 1);
        assert!(display.buffer().iter().all(|b| *b == 0xA5));
        *trace.borrow_mut() = Trace::default();
        block_on(display.flush()).unwrap();
        assert_frame(&writes(&trace.borrow()), display.buffer());
    }
}

#[test]
fn gpio_failures_are_distinguished_from_spi_errors() {
    for high in [false, true] {
        let trace = Shared::default();
        let mut buffer = [0; BUFFER_SIZE];
        trace.borrow_mut().fail_pin = Some(("reset", high));
        let result = block_on(Ili9341Async::new(
            interface(&trace),
            Pin("reset", trace.clone()),
            &mut buffer,
            Config::default(),
            &mut Delay(trace.clone()),
        ));
        assert!(matches!(result, Err(InitError::ResetPin(Fault))));
        assert_eq!(trace.borrow().attempts, 0);
    }
    let trace = Shared::default();
    let mut buffer = [0; BUFFER_SIZE];
    let mut display = display(&trace, &mut buffer, Config::default());
    for high in [false, true] {
        *trace.borrow_mut() = Trace {
            fail_pin: Some(("dc", high)),
            ..Default::default()
        };
        assert!(matches!(
            block_on(display.flush()),
            Err(SpiError::Dc(Fault))
        ));
    }
}

#[test]
fn failed_init_returns_borrowed_resources_for_retry() {
    // Covers configuration and initial-upload failures.
    for fail_at in [0, 7, 13, 14, 20, 223] {
        let trace = Shared::default();
        trace.borrow_mut().fail_write = Some(fail_at);
        let mut buffer = [0x42; BUFFER_SIZE];
        let mut interface = interface(&trace);
        let mut reset = Pin("reset", trace.clone());
        let result = block_on(Ili9341Async::new(
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
        let display = block_on(Ili9341Async::new(
            &mut interface,
            &mut reset,
            &mut buffer,
            Config::default(),
            &mut Delay(trace.clone()),
        ))
        .unwrap();
        assert_eq!(trace.borrow().attempts, 224);
        assert!(display.buffer().iter().all(|b| *b == 0x42));
    }
}

#[test]
fn cancelled_flush_can_restart_and_release_preserves_memory() {
    let trace = Shared::default();
    let mut buffer = [0x55; BUFFER_SIZE];
    let address = buffer.as_ptr();
    let mut display = display(&trace, &mut buffer, Config::default());
    *trace.borrow_mut() = Trace::default();
    {
        let mut future = pin!(display.flush());
        let mut context = Context::from_waker(Waker::noop());
        for _ in 0..8 {
            assert!(future.as_mut().poll(&mut context).is_pending());
        }
    }
    assert_eq!(trace.borrow().attempts, 7); // One strip completed.
    *trace.borrow_mut() = Trace::default();
    block_on(display.flush()).unwrap();
    assert_frame(&writes(&trace.borrow()), display.buffer());
    let (interface, reset, buffer) = display.release();
    assert_eq!(buffer.as_ptr(), address);
    let (_spi, _dc) = interface.release();
    assert_eq!(reset.0, "reset");
}
