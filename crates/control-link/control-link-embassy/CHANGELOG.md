# Changelog

## 0.1.0 (unreleased)

Renamed the package to `polis-control-link-embassy` and its Rust crate to
`polis_control_link_embassy` under the workspace naming standard.

- Point-to-point framed sending and receiving with persistent receive state.
- Independent transport features and caller-owned buffers.
- Optional `supervisor` feature and `ConnectionHandler` trait retire RX/TX together
  on failure, timeout, send cancellation or application-supplied reset; reconnect
  with fresh adapters without buffering/replaying commands.
- Complete `usb::Runner` owns device execution, readiness, splitting, reset handling
  and reconnection, with cancellation-safe lifecycle callbacks and automatic USB
  disable before restart after cancellation. Requires `serial` + `supervisor`.
