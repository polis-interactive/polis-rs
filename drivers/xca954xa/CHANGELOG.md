# Changelog

## 0.1.0 — Unreleased

- Name the package `polis-xca954xa` and the eight-channel driver `Xca9548a`;
  support the shared PCA9548A/TCA9548A control protocol. Other family variants
  are not yet supported.
- Add portable async PCA9548A control, typed mux addresses and channels, and
  exclusively borrowed channel buses implementing `embedded_hal_async::i2c::I2c`.
- Add optional Embassy shared channel handles with one lock across selection
  and downstream transactions, cached selection, and explicit recovery access.
- Preserve selection/transfer error stages and invalidate uncertain selection
  on failures, cancellation and raw bus access.
- Add three-SSD1306 examples for one task and three independently owning tasks,
  with Pico/Pico 2 link configurations and host routing/concurrency tests.
