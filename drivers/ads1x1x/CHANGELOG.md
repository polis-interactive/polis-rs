# Changelog

## 0.1.0 — unreleased

- Rework the UTSuites ADS1015 driver into six explicitly selected I²C models.
- Add 16-bit sample/threshold handling and model-specific data-rate types.
- Validate multiplexer, PGA, comparator and threshold capabilities before I/O.
- Provide matching blocking/async configuration, one-shot and continuous reads.
- Replace cached register setters and unbounded/edge-only waits with full
  configuration, hardware-status reads and bounded polling. Leave GPIO waiting
  to consumers; clear OS on configuration writes.
- Add mock transactions, cancellation/error recovery tests and usage examples.
