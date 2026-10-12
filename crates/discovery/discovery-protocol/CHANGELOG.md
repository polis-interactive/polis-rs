# Changelog

## Unreleased

Renamed the package to `polis-discovery-protocol` and its Rust crate to
`polis_discovery_protocol` under the workspace naming standard.

Rename the package to `discovery-protocol` and move it under
`crates/discovery/discovery-protocol`. Rust imports use `discovery_protocol`;
the discovery schema is unchanged.

## 0.1.0 Unreleased

Define common discovery schema 1 with a consumer-assigned application identifier,
portable UDP and RS-485 message contracts, fixed wire fixtures and slot vectors.

Added `reply_to`, `Announcement::unsolicited`, and allocation-free RS-485
participant/collection state with conservative reply windows. Wire layouts are
unchanged; clocks and I/O remain in the runtime adapters.
