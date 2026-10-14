# Changelog

## 0.1.0 — Unreleased

- Import `broose-goose/adafruit-max31865-rs` as `polis-max31865`, retaining
  BSD-3-Clause and Adafruit credit/notices with source revision and hashes.
- Replace duplicated blocking/optional-async implementations with one default
  allocation-free async `embedded-hal-async` API, independent of MCU/runtime.
- Validate finite positive calibration and ordered 15-bit ADC thresholds;
  write thresholds in one transaction and expose both readbacks.
- Preserve sensor faults as errors, mask command bits during configuration
  updates, wait 66 ms for conversions, and attempt bias shutdown after failures.
- Add explicit fault-cycle commands/completion, latest-sample reads, configurable
  bias settling, cancellation/recovery documentation and numeric validation.
- Add yielding host regression tests, compiled usage documentation, standalone
  packaging inputs and Pico/Pico 2 Embassy examples in the build matrix.

See README.md for breaking differences from the source crate's 0.1.0 API.
