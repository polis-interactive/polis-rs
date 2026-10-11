# W5500 Ethernet startup

`w5500` is a small `no_std` adapter around Embassy's W5500 driver.
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

The controller BSP (in the upstream Slate project) specializes these types
for SPI0 and the ControlBoard pins. Its existing
`board.ethernet.start(&mut state, mac_address).await` API is unchanged.

## Validation

```sh
cargo build -p w5500
cargo build -p w5500 --target thumbv6m-none-eabi --release
cargo build -p w5500 --target thumbv8m.main-none-eabihf --release
cargo clippy -p w5500 -- -D warnings
```

The library builds cover host and embedded portability; the controller example
also links concrete SPI/GPIO types, startup and runner with the MCU runtime.
These are compilation/link checks, not electrical or Ethernet traffic tests.
The example is not deployable firmware; see its BSP guide for limitations.

## Source and distribution

See [PROVENANCE.md](PROVENANCE.md) for source hashes, extraction changes and
license status. Product BSPs and bench firmware remain in their original projects.
