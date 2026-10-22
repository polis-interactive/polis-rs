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
| [polis-framing](crates/protocol-framing) | Allocation-free byte framing with typed payload support |
| [polis-led-output](crates/led-output) | Group fixed-length RGB or RGBW LED chains into one output |
| [polis-discovery-protocol](crates/discovery/discovery-protocol) | Common discovery schema with consumer-assigned application identity |
| [polis-discovery-tokio](crates/discovery/discovery-tokio) | Host discovery; UDP by default, optional RS-485 |
| [polis-discovery-embassy](crates/discovery/discovery-embassy) | Embedded discovery; opt-in UDP and RS-485 |
| [polis-control-link-protocol](crates/control-link/control-link-protocol) | Shared control messages, bounded request tracking, and unsolicited events |
| [polis-control-link-tokio](crates/control-link/control-link-tokio) | Point-to-point TCP and host serial control links |
| [polis-control-link-embassy](crates/control-link/control-link-embassy) | Point-to-point UART, USB CDC and TCP control links |
| [polis-ads1x1x](drivers/ads1x1x) | Async I²C driver for ADS1013/1014/1015 and ADS1113/1114/1115 ADCs |
| [polis-joystick](drivers/joystick) | Portable joystick calibration with borrowed ADS1x1x and RP ADC sources |
| [polis-max31865](drivers/max31865) | Async SPI driver for MAX31865 PT100/PT1000 RTD converters |
| [polis-ssd1306](drivers/ssd1306) | Async I²C 128×32 OLED adapter with a Mousefood/Ratatui example |
| [polis-ili9341](drivers/ili9341) | Async SPI/PIO-parallel 320×240 RGB565 display with Mousefood/Ratatui examples |
| [polis-sh1106](drivers/sh1106) | Async I²C 128×64 OLED driver with a Mousefood/Ratatui example |
| [polis-tmp1x2](drivers/tmp1x2) | Async I²C driver for TMP102 and TMP112 temperature sensors |
| [polis-modulino-buttons](drivers/modulino-buttons) | Async I²C driver for Arduino Modulino Buttons (ABX00110) and its three LEDs |
| [polis-rs485](drivers/rs485) | Async half-duplex RS-485 with direction and turnaround timing |
| [polis-w5500](drivers/w5500) | W5500 Ethernet startup adapter for Embassy |
| [polis-pio-i8080](drivers/pio/i8080) | Write-only 8-bit 8080 display bus using PIO and DMA |
| [polis-pio-i2c](drivers/pio/i2c) | PIO I²C controller for RP2040 and RP2350A |
| [polis-pio-uart](drivers/pio/uart) | Async PIO UART adapter for RP2040 and RP2350A |
| [polis-pio-encoder](drivers/pio/encoder) | Cumulative quadrature positions independent of CPU drain latency |
| [polis-pio-sk6812](drivers/pio/sk6812) | PIO driver for SK6812 RGBW LED chains |
| [polis-pio-ws2813](drivers/pio/ws2813) | PIO driver for WS2813 RGB LED chains |

Each package owns its API documentation, tests, changelog and source provenance.
Every driver includes `examples/embassy.rs`, a minimal Pico/Pico 2 usage example.
Its README documents wiring, expected output, and the `example-rp2040` /
`example-rp235xa` build features. The embedded build matrix links all examples;
target Clippy checks them too.

Original code is licensed under [MIT](LICENSE-MIT). Imported SK6812/WS2813
drivers retain their MIT notices; PIO I²C and MAX31865 retain BSD-3-Clause.
PIO encoder combines original MIT Rust code with an attributed BSD-3-Clause PIO algorithm.
Each package declares its license and includes the applicable notice. No packages are
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
cargo build -p polis-pio-i2c --features rp2040 --target thumbv6m-none-eabi
```

Host tests exercise codecs, sensor/bus behavior and portable waveform logic.
Discovery adapters provide UART streams and UDP probing over the shared protocol.
Discovery supports UDP and RS-485 bus scheduling. Control-link provides
point-to-point runtime adapters; downstream adoption remains a separate step.
Build results do not qualify hardware.

## Guides

| Guide | Purpose |
| --- | --- |
| [Architecture](docs/architecture.md) | Package boundaries and consumer integration |
| [Development](docs/development.md) | Native/embedded matrix, tooling and examples |
| [Publishing](docs/publishing.md) | Per-package releases and remaining release inputs |
| [Migration](docs/migration.md) | Source findings and adoption status |

## Contributions

External contributions are not currently accepted. Please read
[CONTRIBUTING.md](CONTRIBUTING.md) for the contribution policy.
