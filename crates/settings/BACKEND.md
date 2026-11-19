# Backend review — 2026-11-19

Selected `sequential-storage = "=8.0.2"`, defaults disabled. Reviewed the downloaded
registry source, manifest, changelog, map/item/recovery implementation and README.
Registry source revision: `7b99325d8a4f130813107de5411b20680abc99b2`.

- Rust floor 1.87 is below this repository's tested 1.99 floor. Its required
  dependency is `embedded-storage-async` 0.4.x; the workspace resolves 0.4.2,
  also used by Embassy RP 0.10.0. No heap/runtime dependency is required.
- RP async flash uses READ_SIZE=4, WRITE_SIZE=1, ERASE_SIZE=4096. The wrapper
  validates the range and a four-byte-aligned scratch buffer before construction.
  Ownership of the HAL flash value or an exclusive borrow remains explicit.
- 8.x has the owned `MapStorage` and revised cache APIs. Use an uncached map
  initially; no cache-specific timing assumptions. 8.0.1 fixed a cancellation
  interaction involving map remove after cancelled store; 8.0.2 fixes a queue
  issue. There is no reason to adopt FishLamps's old 4.0.0 unchanged.
- The release changelog declares the 5→6→7→8 major releases disk-compatible
  back to 4.x. This is upstream's format claim, **not** a tested FishLamps import:
  key encoding, geometry, partition layout and application payload must also
  agree. No FishLamps record migration is supplied or claimed.
- Upstream documents CRC-protected item headers/data, cyclic page wear leveling,
  automatic repair and recovery after interrupted operations. Corrupt records
  may be skipped; repair can lose unrecoverable data. Cancelled operations have
  an uncertain commit outcome. Neither wrapper nor backend promises hard timing.

The wrapper adds only versioned typed records, explicit outcome handling,
configuration validation and guarded replacement. No new flash KV algorithm,
journal or checksum scheme is introduced. Record-size validation reserves the
backend's page markers, item header, alignment and two-byte key.

Full-page scratch is deliberate: migration may need to copy other keys or future
records larger than the current application payload. Ordinary saves inspect the
stored bytes first, protecting unsupported envelope/schema versions and invalid
payloads; explicit replacement is the product's migration boundary. This
protection cannot recover records that the backend silently skips after CRC
failure. No unsupported physical flash-format autodetection is promised.

Review evidence and primary references:

- [8.0.2 API and recovery contract](https://docs.rs/sequential-storage/8.0.2/sequential_storage/)
- [Pinned upstream changelog](https://github.com/tweedegolf/sequential-storage/blob/7b99325d8a4f130813107de5411b20680abc99b2/CHANGELOG.md)
- [Pinned upstream source](https://github.com/tweedegolf/sequential-storage/tree/7b99325d8a4f130813107de5411b20680abc99b2/src)
- [Embassy RP 0.10.0 flash](https://docs.rs/embassy-rp/0.10.0/embassy_rp/flash/index.html)

Future backend upgrades require an explicit API/format/recovery review and
repeating fault injection and target validation. Exact pinning does not make
application schemas compatible or replace product qualification.
