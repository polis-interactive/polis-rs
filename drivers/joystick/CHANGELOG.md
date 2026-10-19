# Changelog

## 0.1.0 - Unreleased

- Extract the untracked HDMI joystick implementation into `polis-joystick`,
  preserving AxisSource, Calibration, Reading, Joystick, AdsAxes and RpAxes.
- Preserve native counts, asymmetric normalization, truncation, deadzone,
  inversion, sequential ADC borrows, unchanged errors and complete-pair results.
- Retain independent `ads1x1x` / `rp-adc` features; add optional chip forwarding,
  linked Pico/Pico 2 example, package documentation and workspace matrix coverage.
- Retain source tests and add exact rounding and first-axis failure checks.
  Hardware qualification remains unverified.
