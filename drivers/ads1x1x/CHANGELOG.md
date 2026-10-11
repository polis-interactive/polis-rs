# Changelog

## 0.1.0 — unreleased

- Support six explicitly selected I²C ADC models.
- Add 16-bit sample/threshold handling and model-specific data-rate types.
- Validate multiplexer, PGA, comparator and threshold capabilities before I/O.
- Provide async configuration, one-shot and continuous reads.
- Remove the blocking API and direct `embedded-hal` dependency; the existing
  `_async` methods remain available without feature flags.
- Replace cached register setters and unbounded/edge-only waits with full
  configuration, hardware-status reads and bounded polling. Leave GPIO waiting
  to consumers; clear OS on configuration writes.
- Add mock transactions, cancellation/error recovery tests and usage examples.
