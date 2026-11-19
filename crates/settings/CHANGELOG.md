# Changelog

## 0.1.0 — Unreleased

- Add allocation-free typed/versioned records over pinned sequential-storage 8.0.2.
- Validate caller flash ranges and buffers; preserve unsupported/malformed records
  during ordinary saves and expose explicit replacement/reset operations.
- Return load/save/storage outcomes without product defaults, scheduling or tasks.
- Add fault-injection tests and Pico/Pico 2 examples with linker-reserved storage.
