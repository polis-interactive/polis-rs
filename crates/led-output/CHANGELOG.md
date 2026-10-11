# Changelog

## 0.1.0 Unreleased

Initial shared `led-output` library. See PROVENANCE.md for source attribution.

Add `led_outputs!(lane0, lane1, ...)` for heterogeneous lane groups without exposed
pair nesting. Preserve evaluation order, concurrent writes and cancellation.
