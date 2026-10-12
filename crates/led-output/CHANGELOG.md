# Changelog

## 0.1.0 Unreleased

Renamed the package to `polis-led-output` and its Rust crate to
`polis_led_output` under the workspace naming standard.

Initial shared `led-output` library. See PROVENANCE.md for source attribution.

Add `led_outputs!(lane0, lane1, ...)` for heterogeneous lane groups without exposed
pair nesting. Preserve evaluation order, concurrent writes and cancellation.
