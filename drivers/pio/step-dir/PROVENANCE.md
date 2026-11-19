# Source provenance

Original implementation added on 2026-11-19, under the repository owner's MIT
policy. No downstream firmware, GPL code or third-party PIO program was copied.

Design references inspected:

- [Jan Cumps' pio_stepper_lib](https://github.com/jancumps/pio_stepper_lib): finite
  commands and completion; MIT.
- [Embassy PIO PWM](https://github.com/embassy-rs/embassy/blob/main/embassy-rp/src/pio_programs/pwm.rs)
  and local embassy-rp 0.10.0: resource/API integration.
- [grblHAL](https://github.com/grblHAL/RP2040/blob/master/driverPIO.pio) and
  [Klipper](https://github.com/Klipper3d/klipper/blob/master/src/stepper.c):
  execution-contract references only. Their GPL code is not incorporated.

The command format, cycle-balanced boundary/repeat paths, completion ledger,
starvation latch, bounded delays and PC/Y stop accounting are original. There
is no imported revision or file hash set. References explain design context,
not implementation equivalence. Example/build scaffolding follows existing
polis-rs PIO drivers. No mechanical calibration or brake policy is extracted.
