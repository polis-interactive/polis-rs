# Source provenance

Original MIT implementation added to polis-rs on 2026-11-19 for the shared settings
boundary requested by HC. No FishLamps globals, persistence code or application
records were copied. The FishLamps/HC review informed requirements only.

The flash algorithm is the registry dependency `sequential-storage` 8.0.2
(MIT OR Apache-2.0), source revision
`7b99325d8a4f130813107de5411b20680abc99b2`. It is not vendored or modified.
See BACKEND.md for review details. Tests use an original fault-injectable mock,
not the backend's private `_test` feature or copied backend tests.

The example build script and linker layouts adapt this repository's original MIT
TMC5160 example scaffolding; settings reservations and the boot counter are new.
The package-local LICENSE-MIT is copied from the repository's owner-selected
license. No product board configuration, task or firmware was extracted.

The crates.io name check on 2026-11-19 used `cargo search polis-settings` and
returned no matching packages (the direct registry API returned HTTP 403).
No existing package was found; registry acceptance/ownership must still be
confirmed before publication. No package was published.
