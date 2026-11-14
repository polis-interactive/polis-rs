# Changelog

## 0.1.0 — unreleased

- Add portable async TMC5160/A SPI register configuration and diagnostics for
  external STEP/DIR, with no motion generator, UART or OTP programming.
- Add explicit BTT TMC5160T Plus V1.0 board configuration and validated,
  quantization-reporting RMS current calculations.
- Add pipelined reads, status reporting, explicit fault acknowledgement,
  cancellation/error blocking, backend recovery and partial configuration progress.
- Add mock tests and Pico/Pico 2 diagnostic examples for hardware and PIO SPI.
