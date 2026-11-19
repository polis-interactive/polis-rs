use super::*;
use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use embedded_storage_async::nor_flash::{
    ErrorType, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};
use std::{cell::RefCell, rc::Rc, vec, vec::Vec};

const PAGE: usize = 128;
const REGION: Range<u32> = PAGE as u32..(3 * PAGE) as u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlashError {
    Injected,
}
impl NorFlashError for FlashError {
    fn kind(&self) -> NorFlashErrorKind {
        NorFlashErrorKind::Other
    }
}

#[derive(Clone, Copy)]
enum Fault {
    Error,
    Pending,
    Yield,
}

#[derive(Clone)]
struct State {
    bytes: Vec<u8>,
    steps: usize,
    writes: usize,
    erases: usize,
    fail_at: Option<usize>,
    fault: Fault,
}

#[derive(Clone)]
struct Flash<const READ: usize = 4, const WRITE: usize = 1, const ERASE: usize = PAGE>(
    Rc<RefCell<State>>,
);

impl<const R: usize, const W: usize, const E: usize> Flash<R, W, E> {
    fn new() -> Self {
        let mut bytes = vec![0x5a; PAGE * 4];
        bytes[REGION.start as usize..REGION.end as usize].fill(0xff);
        Self(Rc::new(RefCell::new(State {
            bytes,
            steps: 0,
            writes: 0,
            erases: 0,
            fail_at: None,
            fault: Fault::Error,
        })))
    }

    fn from_bytes(bytes: Vec<u8>) -> Self {
        let flash = Self::new();
        flash.0.borrow_mut().bytes = bytes;
        flash
    }

    async fn step(&self) -> Result<(), FlashError> {
        let fault = {
            let mut state = self.0.borrow_mut();
            let step = state.steps;
            state.steps += 1;
            (state.fail_at == Some(step)).then_some(state.fault)
        };
        match fault {
            Some(Fault::Error) => Err(FlashError::Injected),
            Some(Fault::Pending) => core::future::pending().await,
            Some(Fault::Yield) => {
                let mut yielded = false;
                core::future::poll_fn(|cx| {
                    if yielded {
                        Poll::Ready(Ok(()))
                    } else {
                        yielded = true;
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                })
                .await
            }
            None => Ok(()),
        }
    }

    fn bounds(offset: usize, len: usize) {
        assert!(offset >= REGION.start as usize);
        assert!(offset + len <= REGION.end as usize);
    }

    fn check_guards(&self) {
        let state = self.0.borrow();
        assert!(state.bytes[..PAGE].iter().all(|b| *b == 0x5a));
        assert!(state.bytes[3 * PAGE..].iter().all(|b| *b == 0x5a));
    }
}

impl<const R: usize, const W: usize, const E: usize> ErrorType for Flash<R, W, E> {
    type Error = FlashError;
}
impl<const R: usize, const W: usize, const E: usize> ReadNorFlash for Flash<R, W, E> {
    const READ_SIZE: usize = R;
    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), FlashError> {
        let offset = offset as usize;
        Self::bounds(offset, bytes.len());
        assert!(offset.is_multiple_of(R) && bytes.len().is_multiple_of(R));
        // Match the RP DMA driver's RAM alignment requirement.
        assert!((bytes.as_ptr() as usize).is_multiple_of(4));
        self.step().await?;
        bytes.copy_from_slice(&self.0.borrow().bytes[offset..offset + bytes.len()]);
        Ok(())
    }
    fn capacity(&self) -> usize {
        PAGE * 4
    }
}
impl<const R: usize, const W: usize, const E: usize> NorFlash for Flash<R, W, E> {
    const WRITE_SIZE: usize = W;
    const ERASE_SIZE: usize = E;
    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), FlashError> {
        let offset = offset as usize;
        Self::bounds(offset, bytes.len());
        assert!(offset.is_multiple_of(W) && bytes.len().is_multiple_of(W));
        self.0.borrow_mut().writes += 1;
        // Each byte is a possible torn-program boundary, even inside one word.
        for (index, value) in bytes.iter().enumerate() {
            self.step().await?;
            let mut state = self.0.borrow_mut();
            let old = &mut state.bytes[offset + index];
            assert_eq!(*old & value, *value, "attempted 0 -> 1 program");
            *old &= value;
        }
        Ok(())
    }
    async fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        Self::bounds(from as usize, (to - from) as usize);
        assert!((from as usize).is_multiple_of(PAGE) && (to as usize).is_multiple_of(PAGE));
        self.0.borrow_mut().erases += 1;
        for index in from as usize..to as usize {
            self.step().await?;
            self.0.borrow_mut().bytes[index] = 0xff;
        }
        Ok(())
    }
}

// Futures either finish synchronously on the mock or intentionally suspend at a
// fault boundary. No executor dependency or backend-private test API is needed.
fn run<T>(future: impl Future<Output = T>) -> T {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("unexpected suspension"),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Number<const KEY: u16 = 7, const VERSION: u16 = 1>(u32);
impl<const K: u16, const V: u16> Record for Number<K, V> {
    const KEY: u16 = K;
    const VERSION: u16 = V;
    fn encode(&self, output: &mut [u8]) -> Result<usize, CodecError> {
        output
            .get_mut(..4)
            .ok_or(CodecError::BufferTooSmall)?
            .copy_from_slice(&self.0.to_le_bytes());
        Ok(4)
    }
    fn decode(payload: &[u8]) -> Result<Self, CodecError> {
        Ok(Self(u32::from_le_bytes(
            payload
                .try_into()
                .map_err(|_| CodecError::InvalidEncoding)?,
        )))
    }
}
type Value = Number<7, 1>;

fn seed_raw(flash: Flash, key: u16, bytes: &[u8]) {
    let mut backend = Backend::new(flash, MapConfig::new(REGION), Cache::new_uncached());
    let mut scratch = AlignedBuffer([0; PAGE]);
    run(backend.store_item(&mut scratch.0, &key, &bytes)).unwrap();
}

#[test]
fn missing_roundtrip_wire_format_and_reopen() {
    let flash = Flash::<4, 1>::new();
    let mut scratch = AlignedBuffer([0; PAGE]);
    let mut encoded = [0; 16];
    let mut settings = Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded).unwrap();
    assert_eq!(flash.0.borrow().steps, 0);
    assert_eq!(run(settings.load::<Value>()).unwrap(), Load::Missing);
    assert_eq!(flash.0.borrow().erases, 0);
    run(settings.save(&Value::from(123))).unwrap();
    let (owned, scratch, encoded) = settings.free();
    let mut settings = Settings::new(owned, REGION, scratch, encoded).unwrap();
    assert_eq!(
        run(settings.load::<Value>()).unwrap(),
        Load::Present(Number(123))
    );
    let mut backend = Backend::new(flash.clone(), MapConfig::new(REGION), Cache::new_uncached());
    let mut other_scratch = AlignedBuffer([0; PAGE]);
    let bytes = run(backend.fetch_item::<&[u8]>(&mut other_scratch.0, &7))
        .unwrap()
        .unwrap();
    assert_eq!(bytes, &[1, 1, 0, 123, 0, 0, 0]);
    flash.check_guards();
}

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Number(value)
    }
}

#[test]
fn unsupported_and_invalid_records_are_preserved_including_large_unknown_payloads() {
    let cases = [
        (vec![99], RecordProblem::UnsupportedFormat(99)),
        (vec![1, 2, 0, 12], RecordProblem::UnsupportedSchema(2)),
        (
            {
                let mut b = vec![0; 80];
                b[..3].copy_from_slice(&[1, 9, 0]);
                b
            },
            RecordProblem::UnsupportedSchema(9),
        ),
        (
            vec![],
            RecordProblem::InvalidEncoding(CodecError::InvalidEncoding),
        ),
        (
            vec![1],
            RecordProblem::InvalidEncoding(CodecError::InvalidEncoding),
        ),
        (
            vec![1, 1, 0, 2],
            RecordProblem::InvalidEncoding(CodecError::InvalidEncoding),
        ),
        (
            vec![1, 1, 0, 2, 0, 0, 0, 99],
            RecordProblem::InvalidEncoding(CodecError::InvalidEncoding),
        ),
    ];
    for (bytes, problem) in cases {
        let flash = Flash::<4, 1>::new();
        seed_raw(flash.clone(), Value::KEY, &bytes);
        let before = flash.0.borrow().bytes.clone();
        let mut scratch = AlignedBuffer([0; PAGE]);
        let mut encoded = [0; 8];
        let mut settings =
            Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded).unwrap();
        assert_eq!(
            run(settings.load::<Value>()).unwrap(),
            Load::Unusable(problem)
        );
        assert!(
            matches!(run(settings.save(&Number::<7>(3))), Err(SaveError::ExistingRecord(p)) if p == problem)
        );
        assert_eq!(before, flash.0.borrow().bytes);
    }
}

#[test]
fn explicit_migration_and_partition_reset() {
    let flash = Flash::<4, 1>::new();
    let mut scratch = AlignedBuffer([0; PAGE]);
    let mut encoded = [0; 16];
    let mut settings = Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded).unwrap();
    run(settings.save(&Number::<7>(42))).unwrap();
    run(settings.save(&Number::<8>(99))).unwrap();
    assert_eq!(
        run(settings.load::<Number<7, 2>>()).unwrap(),
        Load::Unusable(RecordProblem::UnsupportedSchema(1))
    );
    let Load::Present(old) = run(settings.load::<Value>()).unwrap() else {
        panic!()
    };
    run(settings.replace(&Number::<7, 2>(old.0 + 1))).unwrap();
    assert_eq!(
        run(settings.load::<Number<7, 2>>()).unwrap(),
        Load::Present(Number(43))
    );
    assert_eq!(
        run(settings.load::<Number<8>>()).unwrap(),
        Load::Present(Number(99))
    );
    run(settings.erase_all()).unwrap();
    assert_eq!(run(settings.load::<Number<7, 2>>()).unwrap(), Load::Missing);
    assert_eq!(run(settings.load::<Number<8>>()).unwrap(), Load::Missing);
    flash.check_guards();
}

#[test]
fn configuration_errors_do_not_touch_flash() {
    let flash = Flash::<4, 1>::new();
    for (range, expected) in [
        (0..0, ConfigError::Range),
        (
            Range {
                start: 256,
                end: 128,
            },
            ConfigError::Range,
        ),
        (128..640, ConfigError::Range),
        (129..384, ConfigError::Geometry),
        (128..383, ConfigError::Geometry),
        (128..256, ConfigError::Geometry),
    ] {
        let mut scratch = AlignedBuffer([0; PAGE]);
        let mut encoded = [0; 16];
        assert!(
            matches!(Settings::new(flash.clone(), range, &mut scratch.0, &mut encoded), Err(e) if e == expected)
        );
    }
    let mut scratch = AlignedBuffer([0; PAGE + 4]);
    let mut encoded = [0; 16];
    assert!(matches!(
        Settings::new(
            flash.clone(),
            REGION,
            &mut scratch.0[..PAGE - 1],
            &mut encoded
        ),
        Err(ConfigError::ScratchTooSmall)
    ));
    assert!(matches!(
        Settings::new(flash.clone(), REGION, &mut scratch.0[1..], &mut encoded),
        Err(ConfigError::ScratchUnaligned)
    ));
    assert!(matches!(
        Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded[..2]),
        Err(ConfigError::RecordBufferTooSmall)
    ));
    assert_eq!(flash.0.borrow().steps, 0);
}

#[test]
fn read_write_and_reset_failures_are_returned() {
    let flash = Flash::<4, 1>::new();
    let mut scratch = AlignedBuffer([0; PAGE]);
    let mut encoded = [0; 16];
    let mut settings = Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded).unwrap();
    flash.0.borrow_mut().fail_at = Some(0);
    assert!(matches!(
        run(settings.load::<Value>()),
        Err(StorageError::Storage {
            value: FlashError::Injected,
            ..
        })
    ));
    assert_eq!(flash.0.borrow().writes, 0);
    assert_eq!(flash.0.borrow().erases, 0);
    let next = flash.0.borrow().steps;
    flash.0.borrow_mut().fail_at = Some(next);
    assert!(matches!(
        run(settings.erase_all()),
        Err(StorageError::Storage { .. })
    ));
    flash.0.borrow_mut().fail_at = None;
    run(settings.save(&Number::<7>(10))).unwrap();
    let next = flash.0.borrow().steps;
    flash.0.borrow_mut().fail_at = Some(next);
    assert!(matches!(
        run(settings.save(&Number::<7>(11))),
        Err(SaveError::Storage(_))
    ));
    flash.0.borrow_mut().fail_at = None;
    assert_eq!(
        run(settings.load::<Value>()).unwrap(),
        Load::Present(Number(10))
    );
}

// Baselines for an ordinary append and a save which forces page migration/erase.
fn baseline(migration: bool) -> (Vec<u8>, u32) {
    let flash = Flash::<4, 1>::new();
    let mut scratch = AlignedBuffer([0; PAGE]);
    let mut encoded = [0; 16];
    let mut settings = Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded).unwrap();
    run(settings.save(&Number::<8>(999))).unwrap();
    run(settings.save(&Number::<7>(0))).unwrap();
    for old in 0..100 {
        let bytes = flash.0.borrow().bytes.clone();
        let erases = flash.0.borrow().erases;
        run(settings.save(&Number::<7>(old + 1))).unwrap();
        if !migration || flash.0.borrow().erases > erases {
            return (bytes, old);
        }
    }
    panic!("did not exercise page migration");
}

fn sweep(migration: bool, fault: Fault, reopen: bool) {
    let (baseline, old) = baseline(migration);
    let flash = Flash::<4, 1>::from_bytes(baseline.clone());
    let mut scratch = AlignedBuffer([0; PAGE]);
    let mut encoded = [0; 16];
    let mut settings = Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded).unwrap();
    run(settings.save(&Number::<7>(old + 1))).unwrap();
    let steps = flash.0.borrow().steps;
    assert!(steps > 0);
    if migration {
        assert!(flash.0.borrow().erases > 0);
    }
    for cut in 0..steps {
        let flash = Flash::<4, 1>::from_bytes(baseline.clone());
        {
            let mut state = flash.0.borrow_mut();
            state.fail_at = Some(cut);
            state.fault = fault;
        }
        let mut scratch = AlignedBuffer([0; PAGE]);
        let mut encoded = [0; 16];
        let mut settings =
            Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded).unwrap();
        match fault {
            Fault::Error => assert!(
                matches!(
                    run(settings.save(&Number::<7>(old + 1))),
                    Err(SaveError::Storage(_))
                ),
                "cut {cut}"
            ),
            Fault::Pending => {
                let value = Number::<7>(old + 1);
                let future = settings.save(&value);
                let mut future = pin!(future);
                assert!(
                    future
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop()))
                        .is_pending(),
                    "cut {cut}"
                );
                // Drop the suspended future, simulating cancellation during I/O.
            }
            Fault::Yield => unreachable!(),
        }
        flash.0.borrow_mut().fail_at = None;
        if reopen {
            let (flash, scratch, encoded) = settings.free();
            settings = Settings::new(flash, REGION, scratch, encoded).unwrap();
        }
        let value = run(settings.load::<Value>()).unwrap();
        assert!(
            value == Load::Present(Number(old)) || value == Load::Present(Number(old + 1)),
            "cut {cut}: {value:?}"
        );
        assert_eq!(
            run(settings.load::<Number<8>>()).unwrap(),
            Load::Present(Number(999)),
            "other key at cut {cut}"
        );
        // Recovery leaves the store writable, without an erase-all fallback.
        run(settings.save(&Number::<7>(old + 2))).unwrap();
        assert_eq!(
            run(settings.load::<Value>()).unwrap(),
            Load::Present(Number(old + 2))
        );
        flash.check_guards();
    }
}

#[test]
fn interrupted_appends_recover_old_or_new_record() {
    sweep(false, Fault::Error, true);
}
#[test]
fn interrupted_page_migrations_recover_old_or_new_record() {
    sweep(true, Fault::Error, true);
}
#[test]
fn cancelled_appends_recover_on_same_instance() {
    sweep(false, Fault::Pending, false);
}
#[test]
fn cancelled_page_migrations_recover_on_same_instance() {
    sweep(true, Fault::Pending, false);
}

#[test]
fn corruption_never_decodes_a_partial_record() {
    let flash = Flash::<4, 1>::new();
    seed_raw(flash.clone(), 7, &[1, 1, 0, 42, 0, 0, 0]);
    let baseline = flash.0.borrow().bytes.clone();
    // Damage every bit of the item header and key/envelope/payload, independently.
    // word=4, backend header=8, key+value=9 (padded to 12).
    for byte in PAGE + 4..PAGE + 4 + 8 + 9 {
        for bit in 0..8 {
            let flash = Flash::<4, 1>::from_bytes(baseline.clone());
            flash.0.borrow_mut().bytes[byte] ^= 1 << bit;
            let mut scratch = AlignedBuffer([0; PAGE]);
            let mut encoded = [0; 16];
            let mut settings =
                Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded).unwrap();
            match run(settings.load::<Value>()) {
                Ok(Load::Missing) | Err(StorageError::Corrupted { .. }) => {}
                other => panic!("corruption at {byte}/{bit}: {other:?}"),
            }
            flash.check_guards();
        }
    }
}

#[test]
fn exhaustion_preserves_existing_keys_and_reports_full() {
    let flash = Flash::<4, 1>::new();
    let mut scratch = AlignedBuffer([0; PAGE]);
    let mut encoded = [0; 16];
    let mut settings = Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded).unwrap();
    run(settings.save(&Number::<7>(10))).unwrap();
    // Fill remaining live capacity through the public backend with distinct keys.
    let (flash, scratch, encoded) = settings.free();
    let mut backend = Backend::new(flash.clone(), MapConfig::new(REGION), Cache::new_uncached());
    let mut filled = false;
    for key in 100..200 {
        match run(backend.store_item(scratch, &key, &&[1u8, 1, 0, 1, 0, 0, 0][..])) {
            Ok(()) => {}
            Err(StorageError::FullStorage) => {
                filled = true;
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(filled);
    let mut settings = Settings::new(backend.destroy().0, REGION, scratch, encoded).unwrap();
    assert!(matches!(
        run(settings.save(&Number::<9>(5))),
        Err(SaveError::Storage(StorageError::FullStorage))
    ));
    assert_eq!(
        run(settings.load::<Value>()).unwrap(),
        Load::Present(Number(10))
    );
    flash.check_guards();
}

#[test]
fn encoding_failure_does_not_write() {
    let flash = Flash::<4, 1>::new();
    let mut scratch = AlignedBuffer([0; PAGE]);
    let mut encoded = [0; 6];
    let mut settings = Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded).unwrap();
    assert!(matches!(
        run(settings.save(&Number::<7>(4))),
        Err(SaveError::Encoding(CodecError::BufferTooSmall))
    ));
    assert_eq!(flash.0.borrow().writes, 0);
    assert_eq!(flash.0.borrow().erases, 0);
}

#[test]
fn supports_read_larger_than_write_and_write_larger_than_read() {
    fn roundtrip<const R: usize, const W: usize>() {
        let flash = Flash::<R, W>::new();
        let mut scratch = AlignedBuffer([0; PAGE]);
        let mut encoded = [0; 16];
        let mut settings = Settings::new(flash, REGION, &mut scratch.0, &mut encoded).unwrap();
        run(settings.save(&Number::<7>(123))).unwrap();
        assert_eq!(
            run(settings.load::<Value>()).unwrap(),
            Load::Present(Number(123))
        );
    }
    roundtrip::<4, 1>();
    roundtrip::<1, 4>();
    roundtrip::<4, 4>();
    roundtrip::<32, 32>();
}

#[test]
fn too_small_pages_and_unsupported_word_sizes_are_rejected() {
    let mut scratch = AlignedBuffer([0; PAGE]);
    let mut encoded = [0; 16];
    // Backend permits this page, but a settings key + envelope cannot fit.
    assert!(matches!(
        Settings::new(
            Flash::<1, 1, 11>::new(),
            0..22,
            &mut scratch.0,
            &mut encoded
        ),
        Err(ConfigError::Geometry)
    ));
    assert!(matches!(
        Settings::new(Flash::<64, 64>::new(), REGION, &mut scratch.0, &mut encoded),
        Err(ConfigError::Geometry)
    ));
}

struct Blob<const N: usize>;
impl<const N: usize> Record for Blob<N> {
    const KEY: u16 = 12;
    const VERSION: u16 = 1;
    fn encode(&self, output: &mut [u8]) -> Result<usize, CodecError> {
        output
            .get_mut(..N)
            .ok_or(CodecError::BufferTooSmall)?
            .fill(0x5c);
        Ok(N)
    }
    fn decode(payload: &[u8]) -> Result<Self, CodecError> {
        if payload.len() == N && payload.iter().all(|b| *b == 0x5c) {
            Ok(Self)
        } else {
            Err(CodecError::InvalidEncoding)
        }
    }
}

#[test]
fn maximum_record_fits_and_oversize_never_writes() {
    // 128 - 2*4 page markers - 8 backend header - 2 key - 3 envelope = 107.
    let flash = Flash::<4, 1>::new();
    let mut scratch = AlignedBuffer([0; PAGE]);
    let mut encoded = [0; PAGE];
    let mut settings = Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded).unwrap();
    assert!(matches!(
        run(settings.save(&Blob::<108>)),
        Err(SaveError::RecordTooLarge)
    ));
    assert_eq!(flash.0.borrow().writes, 0);
    run(settings.save(&Blob::<107>)).unwrap();
    assert!(matches!(
        run(settings.load::<Blob<107>>()),
        Ok(Load::Present(_))
    ));
    flash.check_guards();
}

#[test]
fn page_migration_preserves_other_keys_with_unknown_schemas() {
    let flash = Flash::<4, 1>::new();
    let mut unknown = [0x43; 40];
    unknown[..3].copy_from_slice(&[1, 99, 0]);
    seed_raw(flash.clone(), 8, &unknown);
    let mut scratch = AlignedBuffer([0; PAGE]);
    let mut encoded = [0; 7];
    let mut settings = Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded).unwrap();
    for value in 0..30 {
        run(settings.save(&Number::<7>(value))).unwrap();
    }
    assert!(flash.0.borrow().erases > 0);
    assert_eq!(
        run(settings.load::<Number<8>>()).unwrap(),
        Load::Unusable(RecordProblem::UnsupportedSchema(99))
    );
    let (flash, scratch, _) = settings.free();
    let mut backend = Backend::new(flash, MapConfig::new(REGION), Cache::new_uncached());
    assert_eq!(
        run(backend.fetch_item::<&[u8]>(scratch, &8))
            .unwrap()
            .unwrap(),
        unknown
    );
}

#[test]
fn successful_snapshot_does_not_acknowledge_a_newer_application_generation() {
    let flash = Flash::<4, 1>::new();
    let mut scratch = AlignedBuffer([0; PAGE]);
    let mut encoded = [0; 16];
    let mut settings = Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded).unwrap();
    let mut current = (Number::<7>(10), 1u64);
    let snapshot = Number::<7>(current.0.0);
    let saved_generation = current.1;
    flash.0.borrow_mut().fail_at = Some(0);
    flash.0.borrow_mut().fault = Fault::Yield;
    {
        let mut save = pin!(settings.save(&snapshot));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(save.as_mut().poll(&mut cx).is_pending());
        // Product input changes while the older snapshot is being persisted.
        current = (Number::<7>(20), 2);
        assert!(matches!(save.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    let dirty = current.1 != saved_generation;
    assert!(dirty);
    assert_eq!(
        run(settings.load::<Value>()).unwrap(),
        Load::Present(Number(10))
    );
    run(settings.save(&current.0)).unwrap();
    assert_eq!(
        run(settings.load::<Value>()).unwrap(),
        Load::Present(Number(20))
    );
}

#[test]
fn cancelled_destructive_reset_requires_explicit_retry() {
    let flash = Flash::<4, 1>::new();
    let mut scratch = AlignedBuffer([0; PAGE]);
    let mut encoded = [0; 16];
    let mut settings = Settings::new(flash.clone(), REGION, &mut scratch.0, &mut encoded).unwrap();
    run(settings.save(&Number::<7>(10))).unwrap();
    let next = flash.0.borrow().steps;
    flash.0.borrow_mut().fail_at = Some(next + 5);
    flash.0.borrow_mut().fault = Fault::Pending;
    {
        let mut reset = pin!(settings.erase_all());
        assert!(
            reset
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    flash.0.borrow_mut().fail_at = None;
    run(settings.erase_all()).unwrap();
    assert_eq!(run(settings.load::<Value>()).unwrap(), Load::Missing);
    flash.check_guards();
}
