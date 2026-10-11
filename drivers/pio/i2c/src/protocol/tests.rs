use super::*;
use crate::waveform::tests::Machine;
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use std::{vec, vec::Vec};

fn run<T>(future: impl Future<Output = T>) -> T {
    match core::pin::pin!(future)
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("test bus yielded"),
    }
}

struct TestBus {
    machine: Machine,
    bytes: Vec<u16>,
    packets: Vec<Vec<u16>>,
    fail_byte: Option<usize>,
    read_data: u8,
    offset: u8,
}

impl TestBus {
    fn new(offset: u8) -> Self {
        Self {
            machine: Machine::new(offset),
            bytes: Vec::new(),
            packets: Vec::new(),
            fail_byte: None,
            read_data: 0x12,
            offset,
        }
    }
}

impl Bus for TestBus {
    fn scl_offset(&self) -> u8 {
        self.offset
    }
    async fn exchange(&mut self, words: &[u16]) -> Result<u32, Error> {
        if words.len() == 1 {
            let nack = self.fail_byte == Some(self.bytes.len());
            self.bytes.push(words[0]);
            self.machine.samples(self.read_data, nack);
            self.read_data = self.read_data.wrapping_add(1);
        } else {
            self.packets.push(words.to_vec());
        }
        Ok(self.machine.exchange(words))
    }
}

#[test]
fn write_read_has_repeated_start_address_direction_and_final_nack() {
    for offset in [1, 31, 12] {
        let mut bus = TestBus::new(offset);
        let mut data = [0; 2];
        run(transaction(
            &mut bus,
            0x48,
            &mut [Operation::Write(&[0]), Operation::Read(&mut data)],
        ))
        .unwrap();
        assert_eq!(
            bus.bytes,
            vec![
                waveform::byte(0x90, true),
                waveform::byte(0, true),
                waveform::byte(0x91, true),
                waveform::byte(0xff, false),
                waveform::byte(0xff, true)
            ]
        );
        assert_eq!(data, [0x15, 0x16]);
        assert_eq!(bus.packets.len(), 3, "START, repeated START, STOP");
        assert_ne!(bus.packets[0], bus.packets[1]);
        assert!(bus.machine.scl && bus.machine.sda);
        assert_eq!(bus.machine.incoming.len(), 0);
        let conditions: Vec<_> = bus
            .machine
            .sda_edges
            .iter()
            .filter(|e| e.1)
            .map(|e| e.2)
            .collect();
        assert_eq!(
            conditions,
            [false, false, true],
            "START, repeated START, STOP on the pins"
        );
    }
}

#[test]
fn start_stop_hold_and_bus_free_time_survive_stretching() {
    for stretch in [0, 100] {
        let mut bus = TestBus::new(31);
        bus.machine.stretch = stretch;
        let mut data = [0];
        run(transaction(
            &mut bus,
            0x48,
            &mut [Operation::Write(&[0]), Operation::Read(&mut data)],
        ))
        .unwrap();
        let m = &bus.machine;
        for &(time, _, high) in m.sda_edges.iter().filter(|e| e.1) {
            if high {
                assert!(m.cycles - time >= 20, "STOP includes bus-free interval");
                let last_rise = m.edges.iter().rev().find(|e| e.0 <= time && e.1).unwrap().0;
                assert!(
                    time - last_rise >= 20,
                    "STOP setup after physical SCL rises"
                );
            } else {
                let next_fall = m.edges.iter().find(|e| e.0 >= time && !e.1).unwrap().0;
                assert!(next_fall - time >= 20, "START hold before SCL falls");
                if let Some(last_rise) = m.edges.iter().rev().find(|e| e.0 <= time && e.1) {
                    assert!(
                        time - last_rise.0 >= 20,
                        "repeated START setup after stretch"
                    );
                }
            }
        }
    }
}

#[test]
fn adjacent_operations_are_one_contiguous_transfer_in_each_direction() {
    let mut bus = TestBus::new(31);
    let mut a = [0; 1];
    let mut b = [0; 2];
    run(transaction(
        &mut bus,
        0x49,
        &mut [
            Operation::Write(&[1]),
            Operation::Write(&[]),
            Operation::Write(&[2]),
            Operation::Read(&mut a),
            Operation::Read(&mut b),
            Operation::Write(&[3]),
        ],
    ))
    .unwrap();
    assert_eq!(bus.packets.len(), 4);
    assert_eq!(
        bus.bytes,
        vec![
            waveform::byte(0x92, true),
            waveform::byte(1, true),
            waveform::byte(2, true),
            waveform::byte(0x93, true),
            waveform::byte(0xff, false),
            waveform::byte(0xff, false),
            waveform::byte(0xff, true),
            waveform::byte(0x92, true),
            waveform::byte(3, true)
        ]
    );
}

#[test]
fn every_write_nack_including_last_byte_is_reported_and_stopped() {
    for (fail, source) in [
        (0, NoAcknowledgeSource::Address),
        (1, NoAcknowledgeSource::Data),
        (2, NoAcknowledgeSource::Data),
    ] {
        let mut bus = TestBus::new(1);
        bus.fail_byte = Some(fail);
        assert_eq!(
            run(transaction(
                &mut bus,
                0x48,
                &mut [Operation::Write(&[1, 2])]
            )),
            Err(Error::NoAcknowledge(source))
        );
        assert_eq!(bus.bytes.len(), fail + 1);
        assert_eq!(bus.packets.len(), 2, "STOP after NACK");
        assert!(bus.machine.scl && bus.machine.sda);
        bus.fail_byte = None;
        run(transaction(&mut bus, 0x48, &mut [Operation::Write(&[3])])).unwrap();
    }
}

#[test]
fn empty_write_probes_empty_transaction_does_nothing_invalid_inputs_rejected() {
    let mut bus = TestBus::new(1);
    run(transaction(&mut bus, 0x48, &mut [])).unwrap();
    assert!(bus.packets.is_empty());
    run(transaction(&mut bus, 0x48, &mut [Operation::Write(&[])])).unwrap();
    assert_eq!(bus.bytes, [waveform::byte(0x90, true)]);
    assert_eq!(validate(128, &[]), Err(Error::AddressOutOfRange(128)));
    assert_eq!(
        validate(0x48, &[Operation::Read(&mut [])]),
        Err(Error::InvalidReadBufferLength)
    );
}

#[test]
fn recovery_clocks_stretch_and_finishes_with_both_lines_released() {
    let mut bus = TestBus::new(31);
    bus.machine.stretch = 100;
    run(recover(&mut bus)).unwrap();
    assert_eq!(bus.packets.len(), 10, "nine pulses, then STOP");
    assert_eq!(bus.machine.edges.iter().filter(|e| e.1).count(), 10);
    assert!(bus.machine.scl && bus.machine.sda);
}

#[test]
fn timeout_does_not_enqueue_more_bus_operations() {
    struct TimeoutBus(usize);
    impl Bus for TimeoutBus {
        fn scl_offset(&self) -> u8 {
            31
        }
        async fn exchange(&mut self, _: &[u16]) -> Result<u32, Error> {
            self.0 += 1;
            Err(Error::Timeout)
        }
    }
    let mut bus = TimeoutBus(0);
    assert_eq!(
        run(transaction(&mut bus, 0x48, &mut [Operation::Write(&[0])])),
        Err(Error::Timeout)
    );
    assert_eq!(bus.0, 1);
}
