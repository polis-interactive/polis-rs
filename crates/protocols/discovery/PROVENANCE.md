# Source provenance

Derived on 2026-10-11 from `1017-Slate/SlateCode/crates/protocol` at revision
`5cf31aea6bd683d2ac958ca31714f8b480218c4c`. That package was clean at import.

The source had no license declaration at import. On 2026-10-11, the owner
selected MIT for this shared package; see LICENSE-MIT. The original source
checkout and its recorded hashes are unchanged.

Changes: dedicated discovery crate and flattened discovery modules; new common
protocol identity and UDP default; mandatory consumer-provided application prefix
and configured encode/decode API. This defines Polis discovery schema 1; it does
not claim compatibility with earlier wire schemas.
Message bodies, validation and slot hashing derive from the recorded source;
its independent body fixtures, bounds tests and slot vectors are retained and
extended.

## Original file hashes

| File | SHA-256 |
| --- | --- |
| `Cargo.toml` | `b4df996855f9bce1279e8abefffa39c43585a5e27a9f68ee470765f7f31acc73` |
| `README.md` | `17c5fd309177f198d758c4cfeec9167a2e9d58fc3dd2c1ab64b495bc11e87bea` |
| `src/discovery/mod.rs` | `301c3a7ce5ae088fe38e48e880c3d7bcb681f7341ba0fc6a36c7e3b30cb7e7ba` |
| `src/discovery/rs485.rs` | `5ffb73453f9ff06b95a0eba70eb37e41df54a3d41aa5c25edd2354dcf2e305af` |
| `src/discovery/udp.rs` | `31310861fc672e4cc0df532bc8b3cab7314eb9b83e0acf1196568535ff40c1e8` |
| `src/lib.rs` | `275ea86ff9133d7f0fd4666368a193c736b944db5d7802c9f3512a9634bb625b` |
| `tests/discovery.rs` | `5b7a5050dbc88729d242961c9f657ebc87ffe4a2c4712bdb23d392a5ba2ba944` |
