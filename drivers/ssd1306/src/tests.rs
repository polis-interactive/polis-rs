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
use std::{cell::RefCell, rc::Rc, vec, vec::Vec};

#[derive(Default)]
struct State {
    writes: Vec<(u8, Vec<u8>)>,
    fail_at: Option<usize>,
}

#[derive(Clone, Default)]
struct Bus(Rc<RefCell<State>>);

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
        let mut state = self.0.borrow_mut();
        let index = state.writes.len();
        state.writes.push((address, bytes.to_vec()));
        if state.fail_at == Some(index) {
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
    let mut display = Ssd1306Async::new(Bus::default());
    assert_eq!(display.size(), Size::new(128, 32));
    assert_eq!(display.buffer(), &[0; 512]);
    display
        .draw_iter([
            pixel(0, 0, BinaryColor::On),
            pixel(0, 7, BinaryColor::On),
            pixel(0, 8, BinaryColor::On),
            pixel(127, 31, BinaryColor::On),
            pixel(-1, 0, BinaryColor::On),
            pixel(0, -1, BinaryColor::On),
            pixel(128, 0, BinaryColor::On),
            pixel(131, 31, BinaryColor::On),
            pixel(0, 32, BinaryColor::On),
            pixel(i32::MAX, i32::MIN, BinaryColor::On),
        ])
        .unwrap();
    let mut expected = [0; 512];
    expected[0] = 0x81;
    expected[128] = 0x01;
    expected[511] = 0x80;
    assert_eq!(display.buffer(), &expected);
    display.draw_iter([pixel(0, 0, BinaryColor::Off)]).unwrap();
    expected[0] = 0x80;
    assert_eq!(display.buffer(), &expected);
    assert!(display.into_inner().0.borrow().writes.is_empty());
}

#[test]
fn both_clear_paths_only_change_local_pixels() {
    let mut display = Ssd1306Async::new(Bus::default());
    display.clear(BinaryColor::On).unwrap();
    assert_eq!(display.buffer(), &[0xFF; 512]);
    display.clear(BinaryColor::Off).unwrap();
    assert_eq!(display.buffer(), &[0; 512]);
    display.clear(BinaryColor::On).unwrap();
    display.clear_buffer();
    assert_eq!(display.buffer(), &[0; 512]);
    assert!(display.into_inner().0.borrow().writes.is_empty());
}

fn assert_frame(writes: &[(u8, Vec<u8>)], address: u8, frame: &[u8; 512]) {
    assert_eq!(writes.len(), 35);
    assert!(writes.iter().all(|(addr, _)| *addr == address));
    // SSD1306 horizontal addressing: no SH1106 column offset; exactly four pages.
    assert_eq!(writes[0].1, [0x00, 0x20, 0x00]);
    assert_eq!(writes[1].1, [0x00, 0x21, 0x00, 0x7F]);
    assert_eq!(writes[2].1, [0x00, 0x22, 0x00, 0x03]);
    for (chunk, (_, bytes)) in writes[3..].iter().enumerate() {
        assert_eq!(bytes.len(), 17);
        assert_eq!(bytes[0], 0x40);
        assert_eq!(bytes[1..], frame[chunk * 16..(chunk + 1) * 16]);
    }
}

#[test]
fn flush_transmits_exact_visible_pages_to_both_addresses() {
    for address in [Address::Primary, Address::Secondary] {
        let mut display = Ssd1306Async::with_address(Bus::default(), address);
        // A distinct pattern on each page catches stride/offset mistakes.
        for y in 0..32 {
            display
                .draw_iter([pixel(y * 2, y, BinaryColor::On)])
                .unwrap();
        }
        let expected = *display.buffer();
        block_on(display.flush()).unwrap();
        block_on(display.flush()).unwrap();
        let bus = display.into_inner();
        assert_frame(&bus.0.borrow().writes[..35], address as u8, &expected);
        assert_frame(&bus.0.borrow().writes[35..], address as u8, &expected);
    }
}

#[test]
fn init_selects_32_rows_sequential_com_and_internal_pump() {
    let bus = Bus::default();
    let mut display = Ssd1306Async::with_address(bus.clone(), Address::Secondary);
    display
        .draw_iter([pixel(127, 31, BinaryColor::On)])
        .unwrap();
    let expected = *display.buffer();
    block_on(display.init()).unwrap();
    let state = bus.0.borrow();
    assert!(state.writes.iter().all(|(address, _)| *address == 0x3D));
    assert_eq!(state.writes[0].1, [0x00, 0xAE]);
    let commands = &state.writes[..state.writes.len() - 35];
    for required in [
        vec![0, 0xA8, 0x1F],
        vec![0, 0xDA, 0x02],
        vec![0, 0x8D, 0x14],
        vec![0, 0x20, 0x00],
        vec![0, 0x2E],
    ] {
        assert!(
            commands.iter().any(|(_, bytes)| *bytes == required),
            "missing {required:?}"
        );
    }
    assert_eq!(commands.last().unwrap().1, [0, 0xAF]);
    assert_frame(&state.writes[commands.len()..], 0x3D, &expected);
}

#[test]
fn flush_failure_at_every_transfer_preserves_pixels_and_allows_full_retry() {
    for fail_at in 0..35 {
        let bus = Bus::default();
        bus.0.borrow_mut().fail_at = Some(fail_at);
        let mut display = Ssd1306Async::new(bus.clone());
        display.clear(BinaryColor::On).unwrap();
        assert!(matches!(
            block_on(display.flush()),
            Err(DisplayError::BusWriteError)
        ));
        assert_eq!(display.buffer(), &[0xFF; 512]);
        assert_eq!(bus.0.borrow().writes.len(), fail_at + 1);
        bus.0.borrow_mut().fail_at = None;
        bus.0.borrow_mut().writes.clear();
        block_on(display.flush()).unwrap();
        assert_frame(&bus.0.borrow().writes, 0x3C, &[0xFF; 512]);
    }
}

#[test]
fn init_failure_at_every_transfer_allows_reinitialization_without_losing_pixels() {
    let mut reference = Ssd1306Async::new(Bus::default());
    block_on(reference.init()).unwrap();
    let transfers = reference.into_inner().0.borrow().writes.len();
    for fail_at in 0..transfers {
        let bus = Bus::default();
        bus.0.borrow_mut().fail_at = Some(fail_at);
        let mut display = Ssd1306Async::new(bus.clone());
        display.clear(BinaryColor::On).unwrap();
        assert!(matches!(
            block_on(display.init()),
            Err(DisplayError::BusWriteError)
        ));
        assert_eq!(bus.0.borrow().writes.len(), fail_at + 1);
        assert_eq!(display.buffer(), &[0xFF; 512]);
        bus.0.borrow_mut().fail_at = None;
        bus.0.borrow_mut().writes.clear();
        block_on(display.init()).unwrap();
        let state = bus.0.borrow();
        assert_eq!(state.writes.len(), transfers);
        assert_frame(&state.writes[transfers - 35..], 0x3C, &[0xFF; 512]);
    }
}

#[test]
fn cancelling_each_transfer_keeps_pixels_and_retry_resets_the_window() {
    // Polling once starts the first transfer; each subsequent poll completes
    // one transfer and suspends inside the next. No real bus cancellation claim.
    for pending_transfer in 0..35 {
        let bus = Bus::default();
        let mut display = Ssd1306Async::new(bus.clone());
        display
            .draw_iter([pixel(127, 31, BinaryColor::On)])
            .unwrap();
        let expected = *display.buffer();
        {
            let mut future = pin!(display.flush());
            let mut context = Context::from_waker(Waker::noop());
            for _ in 0..=pending_transfer {
                assert!(future.as_mut().poll(&mut context).is_pending());
            }
        }
        assert_eq!(bus.0.borrow().writes.len(), pending_transfer);
        assert_eq!(display.buffer(), &expected);
        bus.0.borrow_mut().writes.clear();
        block_on(display.flush()).unwrap();
        assert_frame(&bus.0.borrow().writes, 0x3C, &expected);
    }
}

#[test]
fn borrowed_bus_is_returned_on_release() {
    let mut bus = Bus::default();
    let mut display = Ssd1306Async::new(&mut bus);
    block_on(display.flush()).unwrap();
    let returned = display.into_inner();
    assert_eq!(returned.0.borrow().writes.len(), 35);
}
