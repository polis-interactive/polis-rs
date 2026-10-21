# Source provenance

Original Rust implementation added to polis-rs on 2026-10-21 for Arduino
Modulino Buttons, SKU ABX00110. Licensed MIT under the repository's policy for
original code. No Arduino C/C++ source is included or translated into this crate;
the sources below establish the device's wire contract.

## Protocol references

- [Arduino_Modulino `src/Modulino.h`](https://github.com/arduino-libraries/Arduino_Modulino/blob/24d08a94ac4e8f83aed211df0fa6f38e9994b164/src/Modulino.h),
  revision `24d08a94ac4e8f83aed211df0fa6f38e9994b164`: `Module::read`,
  `Module::write`, address conversion and `ModulinoButtons`. This reference
  declares Copyright (c) 2025 Arduino SA, SPDX MPL-2.0.
- [node_modulino_firmware `Core/Src/main.c`](https://github.com/arduino/node_modulino_firmware/blob/7d578e0066f741bdad17ce758f91cb62ef81a88a/Core/Src/main.c),
  revision `7d578e0066f741bdad17ce758f91cb62ef81a88a`: `NODE_BUTTONS`,
  `populateSendBuffer`, `getReceiveBufferSize`, `processReceivedData` and the
  I²C callbacks. This reference declares Copyright (c) 2024 Arduino SA and
  refers to its repository license. No firmware is bundled.
- [Arduino ABX00110 datasheet](https://docs.arduino.cc/resources/datasheets/ABX00110-datasheet.pdf),
  consulted 2026-10-21: 3.3 V Qwiic wiring and optional, unpopulated pull-ups.

The firmware returns the fixed hardware pinstrap identifier before A/B/C levels
and accepts exactly three LED bytes. Its address can differ from the pinstrap
identifier after external reconfiguration. The driver uses the normal seven-bit
HAL address (0x3e by default), validates the identifier and has no register writes
before reading. Host-side debounce and address programming are not implemented.

## Repository scaffolding

Package feature configuration, build/linker support and memory maps follow the
MIT `drivers/tmp1x2` package in this repository. The example is original and
uses its established Embassy I²C setup. Test bus/executor scaffolding follows
that package's MIT unit tests. Product pins, firmware and configuration are not
imported; the example pins are illustrative.

The crates.io API returned HTTP 404 for `polis-modulino-buttons` on 2026-10-21.
This does not reserve the name; recheck before publication. Nothing was published.
