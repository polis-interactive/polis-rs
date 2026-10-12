use polis_discovery_protocol::{Announcement, DeviceId, Discover, Message, reply_to, rs485::*};
use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};
const DEVICE: DeviceId = DeviceId(*b"device01");
fn round() -> DiscoveryRound {
    DiscoveryRound {
        discovery_id: 7,
        seed: 1,
        slot_count: NonZeroU16::new(8).unwrap(),
        slot_duration_us: NonZeroU32::new(100).unwrap(),
        turnaround_us: 30,
    }
}
fn timing() -> Timing {
    Timing {
        receive_margin_us: 20,
        transmit_budget_us: NonZeroU32::new(25).unwrap(),
    }
}

#[test]
fn udp_reply_never_grants_an_rs485_slot() {
    let query = Message::Discover(Discover { discovery_id: 7 });
    assert_eq!(
        reply_to(&query, DEVICE),
        Some(Announcement {
            discovery_id: 7,
            device_id: DEVICE
        })
    );
    assert_eq!(
        reply_to(&Message::Discover(Discover { discovery_id: 0 }), DEVICE),
        None
    );
    assert_eq!(
        reply_to(&Message::Rs485DiscoveryRound(round()), DEVICE),
        None
    );
    assert_eq!(Announcement::unsolicited(DEVICE).discovery_id, 0);
}

#[test]
fn response_window_fits_early_and_late_receive_timestamps() {
    let round = round();
    let opens = u64::from(round.turnaround_us) + u64::from(round.slot_for(DEVICE)) * 100;
    let closes = opens + 100;
    for lag in -20i64..=20 {
        let mut participant = Participant::new(DEVICE);
        let window = participant
            .receive(&Message::Rs485DiscoveryRound(round), timing())
            .unwrap();
        // Physical round completion at 1000, observation at 1000+lag.
        assert!(1000 + lag + window.opens_us as i64 >= 1000 + opens as i64);
        assert!(1000 + lag + window.closes_us as i64 <= 1000 + closes as i64);
        assert!(window.opens_us + 25 <= window.closes_us);
    }
    let impossible = Timing {
        receive_margin_us: 38,
        ..timing()
    };
    assert!(
        Participant::new(DEVICE)
            .receive(&Message::Rs485DiscoveryRound(round), impossible)
            .is_none()
    );
    let mut huge = round;
    huge.slot_count = NonZeroU16::new(u16::MAX).unwrap();
    huge.slot_duration_us = NonZeroU32::new(u32::MAX).unwrap();
    huge.turnaround_us = u32::MAX;
    assert!(
        Participant::new(DEVICE)
            .receive(&Message::Rs485DiscoveryRound(huge), timing())
            .is_some()
    );
}

#[test]
fn ack_suppression_retry_seeds_and_new_sessions() {
    let mut participant = Participant::new(DEVICE);
    let mut round = round();
    assert!(
        participant
            .receive(&Message::Rs485DiscoveryRound(round), timing())
            .is_some()
    );
    assert!(
        participant
            .receive(&Message::Rs485DiscoveryRound(round), timing())
            .is_none()
    );
    for ack in [
        DiscoveryAck {
            discovery_id: 8,
            device_id: DEVICE,
        },
        DiscoveryAck {
            discovery_id: 7,
            device_id: DeviceId([0; 8]),
        },
    ] {
        participant.receive(&Message::Rs485DiscoveryAck(ack), timing());
        round.seed += 1;
        assert!(
            participant
                .receive(&Message::Rs485DiscoveryRound(round), timing())
                .is_some()
        );
    }
    participant.receive(
        &Message::Rs485DiscoveryAck(DiscoveryAck {
            discovery_id: 7,
            device_id: DEVICE,
        }),
        timing(),
    );
    round.seed += 1;
    assert!(
        participant
            .receive(&Message::Rs485DiscoveryRound(round), timing())
            .is_none()
    );
    round.discovery_id = 8;
    assert!(
        participant
            .receive(&Message::Rs485DiscoveryRound(round), timing())
            .is_some()
    );
    round.discovery_id = 0;
    assert!(
        participant
            .receive(&Message::Rs485DiscoveryRound(round), timing())
            .is_none()
    );
}

#[test]
fn bounded_collection_only_acknowledges_retained_matching_identities() {
    let id = NonZeroU64::new(7).unwrap();
    let mut collection = Collection::<1>::new(id);
    let reply = Announcement {
        discovery_id: 7,
        device_id: DEVICE,
    };
    assert!(!collection.record(Announcement {
        discovery_id: 0,
        ..reply
    }));
    assert!(collection.record(reply));
    assert!(!collection.record(reply));
    assert!(!collection.record(Announcement {
        device_id: DeviceId([1; 8]),
        ..reply
    }));
    assert_eq!(
        collection.summary(),
        ScanSummary {
            found: 1,
            overflow: true
        }
    );
    assert_eq!(
        collection.acknowledgements().collect::<Vec<_>>(),
        [DiscoveryAck {
            discovery_id: 7,
            device_id: DEVICE
        }]
    );
    let mut empty = Collection::<0>::new(id);
    assert!(!empty.record(reply));
    assert_eq!(
        empty.summary(),
        ScanSummary {
            found: 0,
            overflow: true
        }
    );
    assert_eq!(empty.acknowledgements().count(), 0);
}
