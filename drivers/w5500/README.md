# W5500 Ethernet startup

`polis-w5500` is a small `no_std` adapter around Embassy's W5500 driver.
It owns the reusable startup API without selecting an MCU, SPI peripheral, DMA
channels or pins. Another board using W5500 can construct the same driver with
its own hardware types.

`W5500<SPI, INT, RST>::new(spi, interrupt, reset)` takes ownership without I/O.
`SPI` must implement async `SpiDevice`, `INT` async `Wait`, and `RST` `OutputPin`
to call `start`. The SPI device can be an `ExclusiveDevice` or another adapter
that manages chip select correctly. BSP owns SPI mode/rate, active-low CS,
interrupt input configuration and initial reset level.

`start(&mut state, mac_address).await` consumes the driver and returns
`(Device, Runner)`. Firmware supplies `State<RX, TX>` packet storage and a valid,
unique unicast MAC. Both queues must have at least one slot; zero slots panic.
Use static packet storage for spawned tasks. Firmware passes `Device` to a
compatible network stack and runs `Runner::run()` in a task. IP/DHCP/socket policy
and task spawning stay in firmware.

Chip initialization and packet handling delegate to `embassy-net-wiznet` 0.3.
That driver selects W5500 MACRAW mode, holds reset low for 1 ms and waits 100 ms
after release. It requires an Embassy time driver supplied by the application
and uses `embassy-net-driver` 0.2. SPI and chip-version errors pass through as
`InitError<SPI::Error>`; upstream reset GPIO errors are ignored, so use an
infallible GPIO output where possible. Hardware socket offload is not exposed.

## Minimal Embassy example

[`examples/embassy.rs`](examples/embassy.rs) is a standalone Pico/Pico 2
usage example with all setup in one file.

Starts a W5500 and logs received raw Ethernet frame lengths over RTT.
Connect SPI0 GP18→SCK, GP19→MOSI, GP16←MISO, GP17→CS, GP20←INT, and
GP21→RESET. Use a W5500 module with 3.3 V-compatible logic, its specified
supply, and common ground. SPI runs at 8 MHz, mode 0. Assign a unique
locally administered MAC per board and connect Ethernet to an active network.

The example runs the W5500 runner concurrently with a raw frame consumer;
broadcast traffic such as ARP makes reception visible. It does not configure
IP, answer ping, or open sockets. For those, pass the returned device to
`embassy_net::new` and run the network runner alongside the W5500 runner.
Joining futures in one task allows local packet storage; separately spawned
tasks need static storage.

Build from the workspace root (choose one board):

```sh
# Pico / RP2040
cargo build -p polis-w5500 --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release --locked
# Pico 2 / RP2350A
cargo build -p polis-w5500 --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

The ELF is `.target/<target>/release/examples/embassy`. Flash the matching
image with your usual SWD/UF2 workflow; an RTT-capable debug probe displays
`defmt` output. These commands build and link only; they do not flash hardware.
The opt-in features supply the executor, time driver, panic logger, boot image,
and package-local Pico/Pico 2 memory layout. Normal library builds keep their
existing runtime and MCU choices. Enable only one example board feature.
Examples stop on I/O errors with `unwrap()` to keep the usage path short;
applications own deadlines and recovery. Builds do not qualify hardware.

## Validation

```sh
cargo build -p polis-w5500
cargo build -p polis-w5500 --target thumbv6m-none-eabi --release
cargo build -p polis-w5500 --target thumbv8m.main-none-eabihf --release
cargo clippy -p polis-w5500 -- -D warnings
```

The library builds cover host and embedded portability. They do not qualify
electrical behavior or Ethernet traffic. Runtime and linker integration require
a linked application with concrete SPI/GPIO types and an MCU runtime.

## Source and distribution

See [PROVENANCE.md](PROVENANCE.md) for source hashes, extraction changes and
license status.

## License

Licensed under MIT; see [LICENSE-MIT](LICENSE-MIT).
