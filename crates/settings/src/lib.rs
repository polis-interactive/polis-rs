#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

use core::ops::Range;
use embedded_storage_async::nor_flash::NorFlash;
use sequential_storage::{
    cache::{Cache, Uncached},
    map::{MapConfig, MapStorage},
};

/// Backend errors, including the original flash error, exhaustion and corruption.
pub use sequential_storage::Error as StorageError;

const FORMAT_VERSION: u8 = 1;
const HEADER_LEN: usize = 3;
const KEY_LEN: usize = 2;
type Backend<F> = MapStorage<u16, F, Cache<Uncached, Uncached, Uncached, u16>>;

/// Four-byte aligned storage for backend scratch space (including RP DMA reads).
#[repr(align(4))]
pub struct AlignedBuffer<const N: usize>(pub [u8; N]);

/// An application-defined payload codec. No defaults or migration run implicitly.
///
/// Keys must be unique within a partition, and stable across firmware versions.
/// Use a distinct type with the same key for each supported schema version.
pub trait Record: Sized {
    /// Caller-assigned key, serialized as a little-endian `u16`.
    const KEY: u16;
    /// Application schema version, independent of backend and envelope versions.
    const VERSION: u16;
    /// Encode the complete payload, returning the number of initialized bytes.
    /// Encoding must be deterministic; do not include the envelope header.
    fn encode(&self, output: &mut [u8]) -> Result<usize, CodecError>;
    /// Decode the complete payload, rejecting malformed data and trailing bytes.
    /// Product limits and fallback policy remain the caller's responsibility.
    fn decode(payload: &[u8]) -> Result<Self, CodecError>;
}

/// Failures reported by an application codec or envelope decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    /// The supplied encoding buffer is too small.
    BufferTooSmall,
    /// Malformed, truncated, trailing or otherwise invalid data.
    InvalidEncoding,
    /// An application-defined diagnostic code.
    Application(u16),
}

/// A record exists but cannot be interpreted as the requested type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordProblem {
    /// Unknown settings envelope format. The stored bytes are preserved.
    UnsupportedFormat(u8),
    /// Unknown application schema for this key. The stored bytes are preserved.
    UnsupportedSchema(u16),
    /// A recognized envelope or payload is malformed.
    InvalidEncoding(CodecError),
}

/// Successful storage access does not necessarily yield a usable record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Load<T> {
    /// No intact record was found. Backend CRC failures can also produce this.
    Missing,
    /// A fully decoded record, still subject to product validation.
    Present(T),
    /// Intact backend bytes could not be decoded; ordinary save refuses replacement.
    Unusable(RecordProblem),
}

/// Invalid configuration, detected before accessing flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// Empty/reversed range, or range outside the supplied flash's capacity.
    Range,
    /// Unsupported read/write geometry, page size, alignment or page count.
    Geometry,
    /// Scratch must contain at least one complete erase page.
    ScratchTooSmall,
    /// Scratch must start at a four-byte aligned address.
    ScratchUnaligned,
    /// Encoding buffer must hold at least the three-byte envelope header.
    RecordBufferTooSmall,
}

/// A save failed; the caller must retain its unsaved generation.
#[derive(Debug)]
pub enum SaveError<E> {
    /// Existing bytes require an explicit product migration/replacement decision.
    ExistingRecord(RecordProblem),
    /// The new payload could not be encoded.
    Encoding(CodecError),
    /// Envelope, key and payload exceed one backend item's capacity.
    RecordTooLarge,
    /// Backend failure; a write may have committed before the error was observed.
    Storage(StorageError<E>),
}

/// An allocation-free settings store over a caller-owned flash region.
///
/// Uses an uncached backend and two borrowed buffers: one erase-page scratch
/// buffer, and a small encoding buffer sized for the application's largest record.
/// Full-page scratch permits inspecting/migrating unknown records larger than
/// today's encoding buffer. No executor, timer, default value or global is chosen.
///
/// Every async operation, including load, can trigger backend repair and flash
/// writes/erases. Async does not imply yielding or bounded execution time. After
/// cancellation the operation's durability is unknown; subsequent operations use
/// backend recovery. Never clear a newer dirty generation after saving an older
/// snapshot. Exclusive ownership prevents interleaving this store's operations.
pub struct Settings<'a, F: NorFlash> {
    backend: Backend<F>,
    scratch: &'a mut [u8],
    encoded: &'a mut [u8],
    max_record_len: usize,
}

impl<'a, F: NorFlash> Settings<'a, F> {
    /// Validate resources without reading, programming or erasing flash.
    ///
    /// Offsets are relative to the supplied flash/partition, not XIP addresses.
    /// Reserve this region in the product linker script too; runtime capacity
    /// checks cannot detect overlap with an application image.
    ///
    /// Read/write sizes must be powers of two no larger than 32 bytes; erase
    /// size must be a multiple of both. Supply at least two erase pages.
    /// Backends must also satisfy sequential-storage's flash trait requirements.
    pub fn new(
        flash: F,
        range: Range<u32>,
        scratch: &'a mut [u8],
        encoded: &'a mut [u8],
    ) -> Result<Self, ConfigError> {
        let word = F::READ_SIZE.max(F::WRITE_SIZE);
        if !F::READ_SIZE.is_power_of_two()
            || !F::WRITE_SIZE.is_power_of_two()
            || word > 32
            || F::ERASE_SIZE == 0
            || F::ERASE_SIZE > u32::MAX as usize / 2
            || !F::ERASE_SIZE.is_multiple_of(word)
        {
            return Err(ConfigError::Geometry);
        }
        if range.start >= range.end || u64::from(range.end) > flash.capacity() as u64 {
            return Err(ConfigError::Range);
        }
        let config = MapConfig::try_new(range).map_err(|_| ConfigError::Geometry)?;
        if scratch.len() < F::ERASE_SIZE {
            return Err(ConfigError::ScratchTooSmall);
        }
        if !(scratch.as_ptr() as usize).is_multiple_of(4) {
            return Err(ConfigError::ScratchUnaligned);
        }
        if encoded.len() < HEADER_LEN {
            return Err(ConfigError::RecordBufferTooSmall);
        }
        let data_space = F::ERASE_SIZE - 2 * word - Backend::<F>::item_overhead_size() as usize;
        let max_record_len = data_space
            .min(u16::MAX as usize)
            .checked_sub(KEY_LEN)
            .ok_or(ConfigError::Geometry)?;
        if max_record_len < HEADER_LEN {
            return Err(ConfigError::Geometry);
        }
        Ok(Self {
            backend: MapStorage::new(flash, config, Cache::new_uncached()),
            scratch,
            encoded,
            max_record_len,
        })
    }

    /// Read and decode a record without choosing a default or migration.
    ///
    /// Storage errors remain separate from missing, unsupported and malformed
    /// records. The backend may skip CRC-damaged items or return an older intact
    /// value; this API cannot diagnose corruption the backend does not expose.
    pub async fn load<R: Record>(&mut self) -> Result<Load<R>, StorageError<F::Error>> {
        let Some(bytes) = self
            .backend
            .fetch_item::<&[u8]>(&mut self.scratch[..F::ERASE_SIZE], &R::KEY)
            .await?
        else {
            return Ok(Load::Missing);
        };
        Ok(match decode::<R>(bytes) {
            Ok(record) => Load::Present(record),
            Err(problem) => Load::Unusable(problem),
        })
    }

    /// Save one snapshot, refusing to replace unsupported or malformed records.
    ///
    /// `Ok(())` acknowledges only this snapshot. The caller owns generation
    /// tracking, skipping unchanged values, coalescing and retry scheduling.
    /// Errors/cancellation must not clear dirty state or automatically erase.
    pub async fn save<R: Record>(&mut self, record: &R) -> Result<(), SaveError<F::Error>> {
        match self.load::<R>().await.map_err(SaveError::Storage)? {
            Load::Unusable(problem) => return Err(SaveError::ExistingRecord(problem)),
            Load::Missing | Load::Present(_) => {}
        }
        self.replace(record).await
    }

    /// Explicitly replace this key without checking its existing schema/payload.
    ///
    /// Use only after the product authorizes migration or discarding that key's
    /// previous contents. This does not erase the partition or other keys. Decode
    /// an old schema with `load::<OldRecord>()`, validate/migrate in the product,
    /// then call this with the new schema. No multi-key transaction is provided.
    pub async fn replace<R: Record>(&mut self, record: &R) -> Result<(), SaveError<F::Error>> {
        let limit = self.encoded.len();
        // Clear so a buggy codec cannot persist stale bytes from a previous call.
        self.encoded[..limit].fill(0);
        self.encoded[0] = FORMAT_VERSION;
        self.encoded[1..HEADER_LEN].copy_from_slice(&R::VERSION.to_le_bytes());
        let payload_len = record
            .encode(&mut self.encoded[HEADER_LEN..limit])
            .map_err(SaveError::Encoding)?;
        if payload_len > limit - HEADER_LEN || payload_len > self.max_record_len - HEADER_LEN {
            return Err(SaveError::RecordTooLarge);
        }
        let bytes = &self.encoded[..HEADER_LEN + payload_len];
        self.backend
            .store_item(&mut self.scratch[..F::ERASE_SIZE], &R::KEY, &bytes)
            .await
            .map_err(SaveError::Storage)
    }

    /// Explicitly destroy ALL records in the supplied partition.
    ///
    /// Never called by load or save error handling. Errors are returned to the
    /// caller. Cancellation/power loss may leave a partially erased partition;
    /// retry the authorized reset to completion before treating it as empty.
    pub async fn erase_all(&mut self) -> Result<(), StorageError<F::Error>> {
        self.backend.erase_all().await
    }

    /// Release the flash and borrowed buffers; reconstruction uses a fresh cache.
    pub fn free(self) -> (F, &'a mut [u8], &'a mut [u8]) {
        (self.backend.destroy().0, self.scratch, self.encoded)
    }
}

fn decode<R: Record>(bytes: &[u8]) -> Result<R, RecordProblem> {
    let Some(&format) = bytes.first() else {
        return Err(RecordProblem::InvalidEncoding(CodecError::InvalidEncoding));
    };
    if format != FORMAT_VERSION {
        return Err(RecordProblem::UnsupportedFormat(format));
    }
    let Some(version_bytes) = bytes.get(1..HEADER_LEN) else {
        return Err(RecordProblem::InvalidEncoding(CodecError::InvalidEncoding));
    };
    let version = u16::from_le_bytes([version_bytes[0], version_bytes[1]]);
    if version != R::VERSION {
        return Err(RecordProblem::UnsupportedSchema(version));
    }
    R::decode(&bytes[HEADER_LEN..]).map_err(RecordProblem::InvalidEncoding)
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
