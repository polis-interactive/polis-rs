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

| Location | Packages |
| --- | --- |
| `crates/framing`, `crates/led-output` | `framing`, `led-output` |
| `crates/protocols/discovery` | `discovery`: common discovery schema with consumer-assigned application identity |
| `drivers/ads1x1x` | `ads1x1x`: ADS1013/1014/1015 and ADS1113/1114/1115 I²C ADCs |
| `drivers/tmp1x2`, `drivers/rs485`, `drivers/w5500` | `tmp1x2`, `rs485`, `w5500` |
| `drivers/pio/{i2c,uart,sk6812,ws2813}` | `pio-i2c`, `pio-uart`, `pio-sk6812`, `pio-ws2813` |

Each package owns its API documentation, tests, changelog and source provenance.
Existing upstream license notices are retained. Several original libraries still
need an owner-selected public license before release. No packages are published
by this setup, and a Git remote has not been established here.

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
| [Validation](docs/validation.md) | Test, target build, archive and downstream integration results |
