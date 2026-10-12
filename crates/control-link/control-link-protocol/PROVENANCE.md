# Source provenance

Implemented on 2026-10-11 for polis-rs under its owner-selected MIT license.
See LICENSE-MIT. The message envelope, bounded request table and tests are new;
there is no copied legacy wire codec.

Request correlation, Accepted/final replies, session nonces and monotonic request
acceptance were informed by BuildrootImages at
`44bd0068de1d9e64836d29210528b151f302facb`, specifically
`package/polis-wired-control/daemon/src/link.rs` and
`package/polis-wired-control/crates/protocol/src/control.rs`. Those files were
clean when inspected. The latter's Session ordering logic is expressed here as
a separate RequestGate; product power state and commands are excluded. Those
source package manifests did not declare a license at inspection.

ControlCode at `b0173ce5e084da9e00ec338c1572995b73ecc8d5` was also inspected to
identify TCP request/reply needs. No code from that checkout is copied here.

| Reference file | SHA-256 |
| --- | --- |
| BuildrootImages `daemon/src/link.rs` | `2af30297f19f3b4364180bbe6c29d725c1e69b7a38efc197ff745adf295a2730` |
| BuildrootImages `crates/protocol/src/control.rs` | `51c227fb83984f2eeb8c7b88ed5d2efb255f4b53a38c07f65ab4731d9bf002a5` |
| ControlCode `cc-server/src/control.rs` | `c87d1fbc18c8b7d23ae4940e156792745fe9636f2e1d669bd5ea15e77d9e1a5b` |
