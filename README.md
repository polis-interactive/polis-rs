# polis-rs

Reusable Rust libraries and hardware drivers for embedded and host applications.
Products depend on individual packages; product firmware, BSP assemblies, control
messages and deployment configuration remain in their own repositories.

## Build and test

```sh
cargo test --locked
python3 tools/check.py build --platform all
```

Rust 1.99.0 is pinned with rustfmt, Clippy and the RP2040/RP2350 Arm targets.
Python 3.11 or newer runs the shared build matrix. Root Cargo defaults are host
compatible; MCU implementations are enabled through explicit package features.

## Libraries

| Library | Purpose |
| --- | --- |
| [framing](crates/framing) | Allocation-free byte framing with typed payload support |
| [led-output](crates/led-output) | Group fixed-length RGB or RGBW LED chains into one output |
| [discovery](crates/protocols/discovery) | Common discovery schema with consumer-assigned application identity |
| [ads1x1x](drivers/ads1x1x) | Async I²C driver for ADS1013/1014/1015 and ADS1113/1114/1115 ADCs |
| [tmp1x2](drivers/tmp1x2) | Async I²C driver for TMP102 and TMP112 temperature sensors |
| [rs485](drivers/rs485) | Async half-duplex RS-485 with direction and turnaround timing |
| [w5500](drivers/w5500) | W5500 Ethernet startup adapter for Embassy |
| [pio-i2c](drivers/pio/i2c) | PIO I²C controller for RP2040 and RP2350A |
| [pio-uart](drivers/pio/uart) | Async PIO UART adapter for RP2040 and RP2350A |
| [pio-sk6812](drivers/pio/sk6812) | PIO driver for SK6812 RGBW LED chains |
| [pio-ws2813](drivers/pio/ws2813) | PIO driver for WS2813 RGB LED chains |

Each package owns its API documentation, tests, changelog and source provenance.
Every driver includes `examples/embassy.rs`, a minimal Pico/Pico 2 usage example.
Its README documents wiring, expected output, and the `example-rp2040` /
`example-rp235xa` build features. The embedded build matrix links all examples;
target Clippy checks them too.

Original code is licensed under [MIT](LICENSE-MIT). Imported SK6812/WS2813
drivers retain their MIT notices; PIO I²C retains BSD-3-Clause. Each package
declares its license and includes the applicable notice. No packages are
published by this setup, and a Git remote has not been established here.

## Platform selection

```sh
python3 tools/check.py build --platform rp2040
python3 tools/check.py build --platform rp2350
python3 tools/check.py clippy --platform all
```

The platform option selects the target and matching package features from root
workspace metadata. PIO libraries also work with ordinary Cargo commands:

```sh
cargo build -p pio-i2c --features rp2040 --target thumbv6m-none-eabi
```

Host tests exercise codecs, sensor/bus behavior and portable waveform logic.
Discovery owns its wire contract and slot calculation; socket adapters and live
session state machines remain future work. Build results do not qualify hardware.

## Guides

| Guide | Purpose |
| --- | --- |
| [Architecture](docs/architecture.md) | Package boundaries and consumer integration |
| [Development](docs/development.md) | Native/embedded matrix, tooling and examples |
| [Publishing](docs/publishing.md) | Per-package releases and remaining release inputs |
| [Migration](docs/migration.md) | Source findings and adoption status |
| [Validation](docs/validation.md) | Test, target build, archive and link validation results |

## Contributions

External contributions are not currently accepted. Please read
[CONTRIBUTING.md](CONTRIBUTING.md) for the contribution policy.
