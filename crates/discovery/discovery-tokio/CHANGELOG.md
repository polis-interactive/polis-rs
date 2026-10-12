# Changelog

## 0.1.0

Renamed the package to `polis-discovery-tokio` and its Rust crate to
`polis_discovery_tokio` under the workspace naming standard.

UDP announcement/probe roles and RS-485 bus discovery with scheduled responses,
ACK suppression and bounded collection. Optional features: `udp` and `rs485`.
Point-to-point UART/USB serial discovery and its public stream helpers were removed;
connection/session handling belongs to the forthcoming control-link adapters.
Tokio defaults to UDP.
