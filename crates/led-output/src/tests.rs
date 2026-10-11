use super::*;
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use std::{boxed::Box, cell::RefCell, rc::Rc, vec::Vec};

#[derive(Default)]
struct State {
    ready: u8,
    started: Vec<(u8, Vec<RGBW<u8>>)>,
    finished: Vec<u8>,
    cancelled: Vec<u8>,
}
type Shared = Rc<RefCell<State>>;
struct Lane<const N: usize> {
    id: u8,
    state: Shared,
}
struct Guard {
    id: u8,
    state: Shared,
    finished: bool,
}
impl Drop for Guard {
    fn drop(&mut self) {
        if !self.finished {
            self.state.borrow_mut().cancelled.push(self.id);
        }
    }
}
impl<const N: usize> LedOutput for Lane<N> {
    const PIXEL_COUNT: usize = N;
    async fn write(&mut self, pixels: &[RGBW<u8>]) -> Result<(), FrameLengthError> {
        check_length(N, pixels.len())?;
        self.state
            .borrow_mut()
            .started
            .push((self.id, pixels.to_vec()));
        let mut guard = Guard {
            id: self.id,
            state: self.state.clone(),
            finished: false,
        };
        core::future::poll_fn(|_| {
            if self.state.borrow().ready & (1 << self.id) != 0 {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        guard.finished = true;
        self.state.borrow_mut().finished.push(self.id);
        Ok(())
    }
}
fn lane<const N: usize>(id: u8, state: &Shared) -> Lane<N> {
    Lane {
        id,
        state: state.clone(),
    }
}
fn colors<const N: usize>() -> [RGBW<u8>; N] {
    core::array::from_fn(|i| RGBW {
        r: i as u8,
        ..RGBW::default()
    })
}

#[test]
fn one_lane_keeps_its_complete_frame() {
    let state = Shared::default();
    state.borrow_mut().ready = 1;
    let mut output = led_outputs!(lane::<3>(0, &state));
    assert_eq!((output.lane_count(), output.pixel_count()), (1, 3));
    embassy_futures::block_on(output.write(&colors::<3>())).unwrap();
    assert_eq!(state.borrow().started[0].1, colors::<3>());
}

#[test]
fn unequal_lanes_start_concurrently_and_wait_for_the_last_lane() {
    let state = Shared::default();
    let mut output = led_outputs!(lane::<2>(0, &state), lane::<3>(1, &state));
    assert_eq!((output.lane_count(), output.pixel_count()), (2, 5));
    let pixels = colors::<5>();
    let mut future = Box::pin(output.write(&pixels));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        state.borrow().started,
        [(0, pixels[..2].to_vec()), (1, pixels[2..].to_vec())]
    );
    state.borrow_mut().ready = 1;
    assert!(future.as_mut().poll(&mut cx).is_pending());
    assert_eq!(state.borrow().finished, [0]);
    state.borrow_mut().ready = 3;
    assert_eq!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(())));
    assert_eq!(state.borrow().finished, [0, 1]);
}

#[test]
fn third_lane_has_a_fixed_range_in_macro_group() {
    let state = Shared::default();
    state.borrow_mut().ready = 7;
    let mut output = led_outputs!(
        lane::<2>(0, &state),
        lane::<1>(1, &state),
        lane::<3>(2, &state),
    );
    assert_eq!((output.lane_count(), output.pixel_count()), (3, 6));
    let pixels = colors::<6>();
    embassy_futures::block_on(output.write(&pixels)).unwrap();
    assert_eq!(
        state.borrow().started,
        [
            (0, pixels[..2].to_vec()),
            (1, pixels[2..3].to_vec()),
            (2, pixels[3..].to_vec())
        ]
    );
}

#[test]
fn rejects_short_and_long_frames_without_starting_any_lane() {
    let state = Shared::default();
    let mut output = led_outputs!(lane::<2>(0, &state), lane::<3>(1, &state));
    for pixels in [&colors::<4>()[..], &colors::<6>()[..]] {
        assert_eq!(
            embassy_futures::block_on(output.write(pixels)),
            Err(FrameLengthError {
                expected: 5,
                actual: pixels.len()
            })
        );
    }
    assert!(state.borrow().started.is_empty());
}

#[test]
fn cancellation_drops_all_pending_lanes_and_allows_a_new_complete_frame() {
    let state = Shared::default();
    let mut output = led_outputs!(
        lane::<2>(0, &state),
        lane::<1>(1, &state),
        lane::<3>(2, &state),
    );
    let pixels = colors::<6>();
    let mut future = Box::pin(output.write(&pixels));
    assert!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(future);
    assert_eq!(state.borrow().cancelled, [0, 1, 2]);
    state.borrow_mut().ready = 7;
    embassy_futures::block_on(output.write(&pixels)).unwrap();
    assert_eq!(state.borrow().finished, [0, 1, 2]);
    assert_eq!(state.borrow().started.len(), 6);
}

#[test]
fn rgb_frames_preserve_macro_lane_boundaries_and_reject_bad_lengths_before_io() {
    struct RgbLane<const N: usize>(Rc<RefCell<Vec<Vec<RGB8>>>>);
    impl<const N: usize> LedOutput<RGB8> for RgbLane<N> {
        const PIXEL_COUNT: usize = N;
        async fn write(&mut self, pixels: &[RGB8]) -> Result<(), FrameLengthError> {
            check_length(N, pixels.len())?;
            self.0.borrow_mut().push(pixels.to_vec());
            Ok(())
        }
    }
    let writes = Rc::new(RefCell::new(Vec::new()));
    let mut leds = led_outputs!(
        RgbLane::<2>(writes.clone()),
        RgbLane::<1>(writes.clone()),
        RgbLane::<3>(writes.clone()),
    );
    let pixels: [RGB8; 6] = core::array::from_fn(|i| RGB8::new(i as u8, 10, 20));
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(
        Box::pin(leds.write(&pixels[..5])).as_mut().poll(&mut cx),
        Poll::Ready(Err(FrameLengthError {
            expected: 6,
            actual: 5
        }))
    );
    assert!(writes.borrow().is_empty());
    assert_eq!(
        Box::pin(leds.write(&pixels)).as_mut().poll(&mut cx),
        Poll::Ready(Ok(()))
    );
    assert_eq!(
        *writes.borrow(),
        std::vec![
            pixels[..2].to_vec(),
            pixels[2..3].to_vec(),
            pixels[3..].to_vec()
        ]
    );
}

#[test]
fn macro_evaluates_once_in_order_without_shadowing_caller_names() {
    let state = Shared::default();
    state.borrow_mut().ready = 7;
    let mut evaluations = Vec::new();
    let lanes = lane::<3>(2, &state);
    // The constructor name deliberately collides with the public type name.
    struct LedOutputs;
    let _collision = LedOutputs;
    let mut outputs = led_outputs!(
        {
            evaluations.push(0);
            lane::<1>(0, &state)
        },
        {
            evaluations.push(1);
            lane::<2>(1, &state)
        },
        {
            evaluations.push(2);
            lanes
        },
    );
    assert_eq!(evaluations, [0, 1, 2]);
    assert!(state.borrow().started.is_empty());
    embassy_futures::block_on(outputs.write(&colors::<6>())).unwrap();
    assert_eq!(
        state
            .borrow()
            .started
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
}

#[test]
fn macro_supports_eight_lanes_and_single_lane_trailing_comma() {
    let state = Shared::default();
    state.borrow_mut().ready = u8::MAX;
    let one = led_outputs!(lane::<1>(0, &state),);
    assert_eq!(one.lane_count(), 1);
    let mut outputs = led_outputs!(
        lane::<1>(0, &state),
        lane::<2>(1, &state),
        lane::<3>(2, &state),
        lane::<4>(3, &state),
        lane::<5>(4, &state),
        lane::<6>(5, &state),
        lane::<7>(6, &state),
        lane::<8>(7, &state),
    );
    assert_eq!((outputs.lane_count(), outputs.pixel_count()), (8, 36));
    let pixels = colors::<36>();
    embassy_futures::block_on(outputs.write(&pixels)).unwrap();
    let mut start = 0;
    for (index, (id, written)) in state.borrow().started.iter().enumerate() {
        assert_eq!(*id as usize, index);
        let end = start + index + 1;
        assert_eq!(written, &pixels[start..end]);
        start = end;
    }
    assert_eq!(start, 36);
}
