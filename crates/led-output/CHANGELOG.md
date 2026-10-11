# Changelog

## 0.1.0 Unreleased

Initial shared `led-output` library imported from Slate. See PROVENANCE.md for origin.

Add `led_outputs!(lane0, lane1, ...)` for heterogeneous lane groups without exposed
pair nesting. Preserve evaluation order, concurrent writes and cancellation.
