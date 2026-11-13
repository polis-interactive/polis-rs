# Changelog

## 0.1.0 Unreleased

- Add original async PCA9534/TCA9534 and PCA9534A/TCA9534A register driver.
- Validate addresses and pins; expose typed pin operations and whole-port access.
- Use uncached readback for masked updates; preserve underlying bus errors.
- Add explicit initialization that releases pins before loading output latches.
- Add host error/cancellation tests and a Pico/Pico 2 Embassy example.
- Adapt the example to the HC Controller's five-button/three-LED pinout, using
  low-sink/high-impedance LED control through masked direction updates.

Other XCA953x variants are not supported. See PROVENANCE.md for sources.
