# Source provenance

Original polis-rs implementation dated 2026-10-11 under the repository MIT
license. Composes the existing discovery-protocol and framing crates without
copying downstream product code. See LICENSE-MIT.

UDP role helpers and RS-485 scheduling remain in this package. The earlier
point-to-point UART/USB serial adapters were removed from discovery.

The link fixtures reuse the MIT build script and Pico/Pico 2 memory maps from
`drivers/pio/uart`. The UDP fixture uses an in-memory Ethernet driver. The RS-485
fixture reuses the MIT hardware setup from `drivers/rs485/examples/embassy.rs`,
replacing its hello transmission with discovery rounds. These fixtures validate
linking, not physical timing.
