extern crate std;

use super::*;
use core::{
    future::{Future, poll_fn},
    pin::pin,
    task::{Context, Poll, Waker},
};
use embassy_futures::block_on;
use embedded_hal::i2c::Error as _;
use std::{cell::RefCell, rc::Rc, vec, vec::Vec};

#[derive(Debug, PartialEq)]
enum Op {
    Write(Vec<u8>),
    Read(usize),
}

#[derive(Debug)]
struct Transfer {
    address: u8,
    mask: u8,
    operations: Vec<Op>,
}

#[derive(Default)]
struct State {
    mask: u8,
    active: bool,
    transfers: Vec<Transfer>,
    fail_at: Option<usize>,
}

#[derive(Clone, Default)]
struct Bus(Rc<RefCell<State>>);

struct Active(Rc<RefCell<State>>);

impl Drop for Active {
    fn drop(&mut self) {
        self.0.borrow_mut().active = false;
    }
}

impl ErrorType for Bus {
    type Error = ErrorKind;
}

impl I2c for Bus {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), ErrorKind> {
        let fail = {
            let mut state = self.0.borrow_mut();
            assert!(!state.active, "overlapping physical bus operations");
            state.active = true;
            let fail = state.fail_at == Some(state.transfers.len());
            let mask = state.mask;
            state.transfers.push(Transfer {
                address,
                mask,
                operations: operations
                    .iter()
                    .map(|op| match op {
                        Operation::Write(bytes) => Op::Write(bytes.to_vec()),
                        Operation::Read(bytes) => Op::Read(bytes.len()),
                    })
                    .collect(),
            });
            // Apply effects before suspension, including when an error follows.
            // This models a write that reached hardware before cancellation.
            for op in operations {
                match op {
                    Operation::Write(bytes) if address == 0x70 => {
                        assert_eq!(bytes.len(), 1);
                        state.mask = bytes[0];
                    }
                    Operation::Read(bytes) => bytes.fill(state.mask),
                    Operation::Write(_) => {}
                }
            }
            fail
        };
        let _active = Active(self.0.clone());
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
        if fail { Err(ErrorKind::Bus) } else { Ok(()) }
    }
}

fn selections(bus: &Bus) -> Vec<u8> {
    bus.0
        .borrow()
        .transfers
        .iter()
        .filter_map(|t| match (&t.operations[..], t.address) {
            ([Op::Write(bytes)], 0x70) => Some(bytes[0]),
            _ => None,
        })
        .collect()
}

#[test]
fn validates_addresses_and_all_eight_channels() {
    for raw in 0..=u8::MAX {
        assert_eq!(Address::new(raw).is_some(), (0x70..=0x77).contains(&raw));
        assert_eq!(Channel::new(raw).is_some(), raw < 8);
    }
    for pins in 0..8 {
        let address = Address::from_pins(pins & 4 != 0, pins & 2 != 0, pins & 1 != 0);
        assert_eq!(address.as_u8(), 0x70 | pins);
        assert_eq!(Channel::new(pins).unwrap().mask(), 1 << pins);
    }
    assert_eq!(Address::default(), Address::DEFAULT);
}

#[test]
fn construction_is_silent_and_first_use_overrides_unknown_hardware() {
    let bus = Bus::default();
    bus.0.borrow_mut().mask = 0xff;
    let mut mux = Xca9548a::new(bus.clone(), Address::DEFAULT);
    assert!(bus.0.borrow().transfers.is_empty());
    block_on(mux.channel(Channel::Ch0).write(0x3c, &[0xaa])).unwrap();
    block_on(mux.channel(Channel::Ch0).write(0x3c, &[0xbb])).unwrap();
    block_on(mux.channel(Channel::Ch7).write(0x3c, &[0xcc])).unwrap();
    assert_eq!(selections(&bus), [1, 128]);
    let state = bus.0.borrow();
    assert_eq!(
        state
            .transfers
            .iter()
            .map(|t| t.address)
            .collect::<Vec<_>>(),
        [0x70, 0x3c, 0x3c, 0x70, 0x3c]
    );
    assert_eq!(state.transfers.last().unwrap().mask, 128);
}

#[test]
fn control_masks_reads_and_raw_access_keep_cache_coherent() {
    let bus = Bus::default();
    let mut mux = Xca9548a::new(bus.clone(), Address::DEFAULT);
    block_on(async {
        mux.select_channels(3).await.unwrap();
        mux.select(Channel::Ch0).await.unwrap(); // must replace multi-channel mask
        assert_eq!(mux.read_channels().await.unwrap(), 1);
        mux.select(Channel::Ch0).await.unwrap(); // readback is cached
        mux.bus_mut().write(0x70, &[2]).await.unwrap();
        mux.select(Channel::Ch0).await.unwrap();
        mux.disable_all().await.unwrap();
        mux.disable_all().await.unwrap();
    });
    assert_eq!(selections(&bus), [3, 1, 2, 1, 0]);
    assert_eq!(mux.into_inner().0.borrow().mask, 0);
}

#[test]
fn transaction_keeps_repeated_start_operations_together() {
    let bus = Bus::default();
    let mut mux = Xca9548a::new(bus.clone(), Address::DEFAULT);
    let mut read = [0; 3];
    block_on(
        mux.channel(Channel::Ch2)
            .write_read(0x48, &[0x10], &mut read),
    )
    .unwrap();
    assert_eq!(read, [4; 3]);
    let state = bus.0.borrow();
    assert_eq!(state.transfers.len(), 2); // standalone select/STOP, then write-read
    assert_eq!(
        state.transfers[1].operations,
        [Op::Write(vec![0x10]), Op::Read(3)]
    );
}

#[test]
fn channel_rejects_reserved_addresses_and_its_own_mux_without_io() {
    let bus = Bus::default();
    let mut mux = Xca9548a::new(bus.clone(), Address::from_pins(true, true, true));
    for address in (0..8).chain(0x77..=255) {
        assert_eq!(
            block_on(mux.channel(Channel::Ch0).write(address, &[1])),
            Err(Error::InvalidDeviceAddress(address))
        );
    }
    block_on(mux.channel(Channel::Ch0).transaction(0x3c, &mut [])).unwrap();
    assert!(bus.0.borrow().transfers.is_empty());
}

#[test]
fn errors_preserve_stage_and_kind_and_force_reselection() {
    for fail_at in 0..2 {
        let bus = Bus::default();
        bus.0.borrow_mut().fail_at = Some(fail_at);
        let mut mux = Xca9548a::new(bus.clone(), Address::DEFAULT);
        let error = block_on(mux.channel(Channel::Ch0).write(0x3c, &[1])).unwrap_err();
        assert_eq!(
            error,
            if fail_at == 0 {
                Error::Select(ErrorKind::Bus)
            } else {
                Error::Transfer(ErrorKind::Bus)
            }
        );
        assert_eq!(error.kind(), ErrorKind::Bus);
        assert_eq!(bus.0.borrow().transfers.len(), fail_at + 1);
        bus.0.borrow_mut().fail_at = None;
        block_on(mux.channel(Channel::Ch0).write(0x3c, &[2])).unwrap();
        assert_eq!(selections(&bus), [1, 1]);
    }
}

#[test]
fn cancelled_selection_does_not_reuse_previous_cached_channel() {
    let bus = Bus::default();
    let mut mux = Xca9548a::new(bus.clone(), Address::DEFAULT);
    block_on(mux.select(Channel::Ch0)).unwrap();
    {
        let mut future = pin!(mux.select(Channel::Ch1));
        assert!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    assert_eq!(bus.0.borrow().mask, 2);
    block_on(mux.channel(Channel::Ch0).write(0x3c, &[1])).unwrap();
    assert_eq!(selections(&bus), [1, 2, 1]);
}

#[test]
fn cancelled_downstream_transfer_forces_reselection() {
    let bus = Bus::default();
    let mut mux = Xca9548a::new(bus.clone(), Address::DEFAULT);
    {
        let mut channel = mux.channel(Channel::Ch0);
        let mut future = pin!(channel.write(0x3c, &[1]));
        let mut context = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut context).is_pending()); // select
        assert!(future.as_mut().poll(&mut context).is_pending()); // downstream
    }
    block_on(mux.channel(Channel::Ch0).write(0x3c, &[2])).unwrap();
    assert_eq!(selections(&bus), [1, 1]);
}

#[cfg(feature = "embassy")]
mod sharing {
    use super::*;
    use embassy_futures::join::join3;
    use embassy_sync::blocking_mutex::raw::NoopRawMutex;
    use embedded_graphics_core::{
        Pixel, draw_target::DrawTarget, geometry::Point, pixelcolor::BinaryColor,
    };
    use polis_ssd1306::Ssd1306Async;

    type Mux = SharedMux<NoopRawMutex, Bus>;

    #[test]
    fn repeated_handles_and_clones_share_selection_cache() {
        let bus = Bus::default();
        let mux = Mux::new(Xca9548a::new(bus.clone(), Address::DEFAULT));
        let mut a = mux.channel(Channel::Ch0);
        let mut b = a.clone();
        let mut c = mux.channel(Channel::Ch0);
        block_on(async {
            a.write(0x3c, &[1]).await.unwrap();
            b.write(0x48, &[2]).await.unwrap();
            c.write(0x49, &[3]).await.unwrap();
        });
        assert_eq!(selections(&bus), [1]);
    }

    #[test]
    fn concurrent_reads_and_writes_cannot_switch_anothers_route() {
        let bus = Bus::default();
        let mux = Mux::new(Xca9548a::new(bus.clone(), Address::DEFAULT));
        let run = |channel: Channel| {
            let mut handle = mux.channel(channel);
            async move {
                for _ in 0..8 {
                    let mut read = [0; 2];
                    handle
                        .write_read(0x48, &[channel.mask()], &mut read)
                        .await
                        .unwrap();
                    assert_eq!(read, [channel.mask(); 2]);
                    handle.read(0x48, &mut read).await.unwrap();
                    assert_eq!(read, [channel.mask(); 2]);
                }
            }
        };
        block_on(join3(
            run(Channel::Ch0),
            run(Channel::Ch1),
            run(Channel::Ch2),
        ));
        assert_eq!(
            bus.0
                .borrow()
                .transfers
                .iter()
                .filter(|t| t.address == 0x48)
                .count(),
            48
        );
    }

    #[test]
    fn cancellation_releases_lock_during_selection_and_transfer() {
        for polls in 1..=2 {
            let bus = Bus::default();
            let mux = Mux::new(Xca9548a::new(bus.clone(), Address::DEFAULT));
            let mut a = mux.channel(Channel::Ch0);
            let mut b = mux.channel(Channel::Ch1);
            {
                let mut owner = pin!(a.write(0x3c, &[1]));
                let mut waiter = pin!(b.write(0x3c, &[2]));
                let mut context = Context::from_waker(Waker::noop());
                for _ in 0..polls {
                    assert!(owner.as_mut().poll(&mut context).is_pending());
                }
                assert!(waiter.as_mut().poll(&mut context).is_pending());
                // Waiting for the mutex must not access the physical bus.
                assert_eq!(bus.0.borrow().transfers.len(), polls);
            }
            block_on(b.write(0x3c, &[3])).unwrap();
            assert_eq!(bus.0.borrow().transfers.last().unwrap().mask, 2);
        }
    }

    #[test]
    fn cancelled_waiter_does_not_invalidate_owners_successful_selection() {
        let bus = Bus::default();
        let mux = Mux::new(Xca9548a::new(bus.clone(), Address::DEFAULT));
        let mut a = mux.channel(Channel::Ch0);
        let mut b = mux.channel(Channel::Ch1);
        {
            let mut owner = pin!(a.write(0x3c, &[1]));
            let mut context = Context::from_waker(Waker::noop());
            assert!(owner.as_mut().poll(&mut context).is_pending());
            {
                let mut waiter = pin!(b.write(0x3c, &[2]));
                assert!(waiter.as_mut().poll(&mut context).is_pending());
            }
            block_on(owner).unwrap();
        }
        block_on(a.write(0x3c, &[3])).unwrap();
        assert_eq!(selections(&bus), [1]);
    }

    #[test]
    fn reset_and_bus_recovery_are_serialized_with_channel_users() {
        let bus = Bus::default();
        let mux = Mux::new(Xca9548a::new(bus.clone(), Address::DEFAULT));
        block_on(async {
            mux.channel(Channel::Ch0).write(0x3c, &[1]).await.unwrap();
            {
                let mut guard = mux.lock().await;
                bus.0.borrow_mut().mask = 0; // external RESET under the guard
                guard.invalidate_selection();
            }
            mux.channel(Channel::Ch0).write(0x3c, &[2]).await.unwrap();
            mux.lock().await.bus_mut().write(0x70, &[0]).await.unwrap();
            mux.channel(Channel::Ch0).write(0x3c, &[3]).await.unwrap();
        });
        assert_eq!(selections(&bus), [1, 1, 0, 1]);
        assert_eq!(mux.into_inner().into_inner().0.borrow().mask, 1);
    }

    #[test]
    fn three_ssd1306_drivers_keep_complete_frames_on_their_own_channels() {
        let bus = Bus::default();
        let mux = Mux::new(Xca9548a::new(bus.clone(), Address::DEFAULT));
        let run = |channel: Channel| {
            let mut display = Ssd1306Async::new(mux.channel(channel));
            async move {
                display.init().await.unwrap();
                // A different lit row on each panel catches swapped routes.
                display
                    .draw_iter(
                        (0..128).map(|x| Pixel(Point::new(x, channel as i32), BinaryColor::On)),
                    )
                    .unwrap();
                display.flush().await.unwrap();
            }
        };
        block_on(join3(
            run(Channel::Ch0),
            run(Channel::Ch1),
            run(Channel::Ch2),
        ));
        let state = bus.0.borrow();
        for mask in [1, 2, 4] {
            let writes: Vec<_> = state
                .transfers
                .iter()
                .filter(|t| t.address == 0x3c && t.mask == mask)
                .map(|t| match &t.operations[..] {
                    [Op::Write(bytes)] => bytes.as_slice(),
                    _ => panic!("unexpected display transaction"),
                })
                .collect();
            let frame = &writes[writes.len() - 35..];
            assert_eq!(frame[0], [0, 0x20, 0]);
            assert_eq!(frame[1], [0, 0x21, 0, 127]);
            assert_eq!(frame[2], [0, 0x22, 0, 3]);
            for (index, chunk) in frame[3..].iter().enumerate() {
                assert_eq!(chunk.len(), 17);
                assert_eq!(chunk[0], 0x40);
                assert_eq!(chunk[1..], [if index < 8 { mask } else { 0 }; 16]);
            }
        }
        assert!(
            state
                .transfers
                .iter()
                .filter(|t| t.address == 0x3c)
                .all(|t| [1, 2, 4].contains(&t.mask))
        );
    }
}
