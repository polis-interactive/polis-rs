# Changelog

## 0.2.0 (unreleased)

- Expose read-only `half_duplex::Link::transport()` access for backend diagnostics without bypassing framing ownership.

Renamed the package to `polis-control-link-embassy` and its Rust crate to
`polis_control_link_embassy` under the workspace naming standard.

- Point-to-point framed sending and receiving with persistent receive state.
- Independent transport features and caller-owned buffers.
- Always-available `half_duplex::Link` owns one `Read + Write` backend with
  persistent framing, exclusive send/receive, and retirement of both directions
  on interrupted TX or fatal RX. Backend recovery and bus/session policy remain
  explicit and application-owned. Reuses stream framing without wire/API changes.
- Two-board half-duplex RS-485 example with a versioned identity exchange,
  request tracking, reply budgets and explicit recovery; links on Pico/Pico 2.
- Optional `supervisor` feature and `ConnectionHandler` trait retire RX/TX together
  on failure, timeout, send cancellation or application-supplied reset; reconnect
  with fresh adapters without buffering/replaying commands.
- Complete `usb::Runner` owns device execution, readiness, splitting, reset handling
  and reconnection, with cancellation-safe lifecycle callbacks and automatic USB
  disable before restart after cancellation. Requires `serial` + `supervisor`.
