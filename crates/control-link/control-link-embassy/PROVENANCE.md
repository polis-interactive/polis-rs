# Source provenance

Original polis-rs implementation, 2026-10-11, under the repository MIT license.
Composes control-link-protocol and embassy I/O; no downstream product source
is copied. See LICENSE-MIT.

The optional connection supervisor was designed against
`BuildrootImages/package/polis-wired-control/firmware/src/usb.rs` on 2026-10-11.
The consumer checkout was at `475d0a50f2005101cf793bea7c2ec6b3682d1fa0` with an
uncommitted migration to the shared control-link adapters. Its pre-change USB
source SHA-256 was
`07880405e057017936ceb1f1f5fe3fce34c608855e85ec01af1068bd51aab228`.
The shared supervisor is original MIT code. Product queues, generations, USB
handler, identity handshake and power policy were not extracted into this crate.

The complete USB runner was subsequently extracted against the same consumer,
whose pre-runner USB source SHA-256 was
`8c77f28cc925609b19383d706acb9f4b3872157e7c553526408c71e5a234cad2`.
The consumer link task, readiness/reset/reconnect logic and USB event handler
were removed. The library implementation adds latched events and scoped cleanup;
product epoch updates, queue handling, timeout values and power logic remain in
the consumer's lifecycle/connection handlers. The runner is original MIT code.
