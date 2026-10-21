extern crate std;

use super::*;
use core::{
    future::{Future, poll_fn},
    pin::pin,
    task::{Context, Poll, Waker},
};
use embassy_futures::block_on;
use embedded_graphics_core::geometry::Point;
use embedded_hal::i2c::{ErrorKind, ErrorType, Operation};
use std::{vec, vec::Vec};

#[derive(Default)]
struct Bus {
    writes: Vec<(u8, Vec<u8>)>,
    fail_at: Option<usize>,
}

impl ErrorType for Bus {
    type Error = ErrorKind;
}

impl I2c for Bus {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        // Every transfer suspends, so tests exercise actual async state changes.
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
        let [Operation::Write(bytes)] = operations else {
            panic!("expected a single write")
        };
        let index = self.writes.len();
        self.writes.push((address, bytes.to_vec()));
        if self.fail_at == Some(index) {
            return Err(ErrorKind::Bus);
        }
        Ok(())
    }
}

fn pixel(x: i32, y: i32, color: BinaryColor) -> Pixel<BinaryColor> {
    Pixel(Point::new(x, y), color)
}

#[test]
fn visible_geometry_clipping_and_page_bits() {
    let mut display = Sh1106Async::new(Bus::default());
    assert_eq!(display.size(), Size::new(128, 64));
    assert_eq!(display.buffer(), &[0; 1024]);
    display
        .draw_iter([
            pixel(0, 0, BinaryColor::On),
            pixel(0, 7, BinaryColor::On),
            pixel(0, 8, BinaryColor::On),
            pixel(127, 63, BinaryColor::On),
            pixel(-1, 0, BinaryColor::On),
            pixel(0, -1, BinaryColor::On),
            pixel(128, 0, BinaryColor::On),
            pixel(131, 63, BinaryColor::On),
            pixel(0, 64, BinaryColor::On),
            pixel(i32::MAX, i32::MIN, BinaryColor::On),
        ])
        .unwrap();
    let mut expected = [0; 1024];
    expected[0] = 0x81;
    expected[128] = 0x01;
    expected[1023] = 0x80;
    assert_eq!(display.buffer(), &expected);
    display.draw_iter([pixel(0, 0, BinaryColor::Off)]).unwrap();
    expected[0] = 0x80;
    assert_eq!(display.buffer(), &expected);
    assert!(display.into_inner().writes.is_empty());
}

#[test]
fn both_clear_paths_only_change_local_pixels() {
    let mut display = Sh1106Async::new(Bus::default());
    display.clear(BinaryColor::On).unwrap();
    assert_eq!(display.buffer(), &[0xFF; 1024]);
    display.clear(BinaryColor::Off).unwrap();
    assert_eq!(display.buffer(), &[0; 1024]);
    display.clear(BinaryColor::On).unwrap();
    display.clear_buffer();
    assert_eq!(display.buffer(), &[0; 1024]);
    assert!(display.into_inner().writes.is_empty());
}

fn assert_frame(writes: &[(u8, Vec<u8>)], address: u8, frame: &[u8; 1024]) {
    assert_eq!(writes.len(), 32);
    for (page, group) in writes.as_chunks::<4>().0.iter().enumerate() {
        assert!(group.iter().all(|(addr, _)| *addr == address));
        assert_eq!(group[0].1, [0x00, 0xB0 + page as u8]);
        assert_eq!(group[1].1, [0x00, 0x02]);
        assert_eq!(group[2].1, [0x00, 0x10]);
        assert_eq!(group[3].1.len(), 129);
        assert_eq!(group[3].1[0], 0x40);
        assert_eq!(group[3].1[1..], frame[page * 128..(page + 1) * 128]);
    }
}

#[test]
fn flush_transmits_exact_visible_pages_to_both_addresses() {
    for address in [Address::Primary, Address::Secondary] {
        let mut display = Sh1106Async::with_address(Bus::default(), address);
        // A distinct pattern on each page catches stride/offset mistakes.
        for y in 0..64 {
            display
                .draw_iter([pixel(y * 2, y, BinaryColor::On)])
                .unwrap();
        }
        let expected = *display.buffer();
        block_on(display.flush()).unwrap();
        block_on(display.flush()).unwrap();
        let bus = display.into_inner();
        assert_frame(&bus.writes[..32], address as u8, &expected);
        assert_frame(&bus.writes[32..], address as u8, &expected);
    }
}

#[test]
fn init_uses_sh1106_converter_and_uploads_before_enabling_display() {
    let mut display = Sh1106Async::with_address(Bus::default(), Address::Secondary);
    display
        .draw_iter([pixel(127, 63, BinaryColor::On)])
        .unwrap();
    let expected = *display.buffer();
    block_on(display.init()).unwrap();
    let bus = display.into_inner();
    let commands = [
        vec![0x00, 0xAE],
        vec![0x00, 0xD5, 0x80],
        vec![0x00, 0xA8, 0x3F],
        vec![0x00, 0xD3, 0x00],
        vec![0x00, 0x40],
        vec![0x00, 0xAD, 0x8B],
        vec![0x00, 0xA1],
        vec![0x00, 0xC8],
        vec![0x00, 0xDA, 0x12],
        vec![0x00, 0x81, 0x7F],
        vec![0x00, 0xD9, 0x22],
        vec![0x00, 0xDB, 0x20],
        vec![0x00, 0xA4],
        vec![0x00, 0xA6],
    ];
    assert_eq!(bus.writes.len(), 47);
    for ((address, bytes), command) in bus.writes[..14].iter().zip(commands) {
        assert_eq!(*address, 0x3D);
        assert_eq!(*bytes, command);
    }
    assert_frame(&bus.writes[14..46], 0x3D, &expected);
    assert_eq!(bus.writes[46], (0x3D, vec![0x00, 0xAF]));
}

#[test]
fn flush_failure_at_every_transfer_preserves_pixels_and_allows_full_retry() {
    for fail_at in 0..32 {
        let mut display = Sh1106Async::new(Bus {
            fail_at: Some(fail_at),
            ..Default::default()
        });
        display.clear(BinaryColor::On).unwrap();
        assert_eq!(block_on(display.flush()), Err(ErrorKind::Bus));
        assert_eq!(display.buffer(), &[0xFF; 1024]);
        assert_eq!(display.bus_mut().writes.len(), fail_at + 1);
        display.bus_mut().fail_at = None;
        display.bus_mut().writes.clear();
        block_on(display.flush()).unwrap();
        assert_frame(&display.into_inner().writes, 0x3C, &[0xFF; 1024]);
    }
}

#[test]
fn init_stops_on_each_error_and_can_be_repeated() {
    for fail_at in 0..47 {
        let mut display = Sh1106Async::new(Bus {
            fail_at: Some(fail_at),
            ..Default::default()
        });
        assert_eq!(block_on(display.init()), Err(ErrorKind::Bus));
        assert_eq!(display.bus_mut().writes.len(), fail_at + 1);
        display.bus_mut().fail_at = None;
        display.bus_mut().writes.clear();
        block_on(display.init()).unwrap();
        assert_eq!(display.bus_mut().writes.len(), 47);
        assert_eq!(display.bus_mut().writes[0].1, [0x00, 0xAE]);
    }
}

#[test]
fn cancelled_flush_restarts_addressing_and_keeps_framebuffer() {
    let mut display = Sh1106Async::new(Bus::default());
    display
        .draw_iter([pixel(127, 63, BinaryColor::On)])
        .unwrap();
    let expected = *display.buffer();
    {
        let mut future = pin!(display.flush());
        let mut context = Context::from_waker(Waker::noop());
        // Finish one page and suspend in the next page's address transfer.
        for _ in 0..5 {
            assert!(future.as_mut().poll(&mut context).is_pending());
        }
    }
    assert_eq!(display.bus_mut().writes.len(), 4);
    assert_eq!(display.buffer(), &expected);
    display.bus_mut().writes.clear();
    block_on(display.flush()).unwrap();
    assert_frame(&display.into_inner().writes, 0x3C, &expected);
}
