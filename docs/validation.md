# Library validation

## Optional Embassy connection supervisor

Validated on 2026-10-11 with Rust 1.99.0 on branch `bw-control-link`.
`polis-control-link-embassy` adds an optional `supervisor` feature and
`ConnectionHandler<R, W>` trait. Scoped RX/TX adapters share retirement state;
reset, send timeout/cancellation, fatal I/O or either loop ending drops both
halves before returning. Low-level split remains unchanged and `no_std` builds
require no allocation. Queues, identity/session handling, timeout values,
reconnect readiness and power-off policy remain application-owned.

```sh
python3 tools/check.py test --package polis-control-link-embassy
python3 tools/check.py build --platform all --package polis-control-link-embassy
python3 tools/check.py clippy --platform all --package polis-control-link-embassy
RUSTDOCFLAGS='-D warnings' python3 tools/check.py doc --package polis-control-link-embassy
cargo test --locked --offline -p polis-control-link-embassy --no-default-features --features serial,supervisor --doc
```

All sixteen feature combinations passed native tests, builds, Clippy and docs.
RP2040 and RP2350 library builds and target Clippy passed, and the supervised
USB link fixture linked on both targets. Eight added tests cover retirement
when an application ignores an error, cancellation of an individual send or
whole scope, reset precedence, write/flush timeouts, decoder state across
reconnection, and no automatic replay. CDC mocks verify reset during partial
input followed by reborrowing the same endpoints/storage for a new connection.
Existing unsupervised stream/serial tests continue to pass.

The concrete consumer is the locally modified
`../BuildrootImages/package/polis-wired-control/firmware/src/usb.rs` at base
revision `475d0a50f2005101cf793bea7c2ec6b3682d1fa0`. Its receive/transmit business
loops now implement `ConnectionHandler` on `Application { generation }`, replacing
the explicit RX/TX/reset race and separate send timeout wrapper. Only that USB
file and `firmware/Cargo.toml` were changed for this step. Existing downstream
changes were preserved. USB reset generation signalling, queue priorities,
stale-generation filtering, pending-frame loss accounting and the 500 ms send
budget remain in the consumer; handshake and power policy were not moved.

The adapted firmware linked and passed Clippy for `thumbv8m.main-none-eabihf`
with the `firmware` profile in `/tmp/polis-supervisor-consumer`. This is a copy of
the dirty consumer workspace, with temporary Git-source patches pointing to the
local polis-rs packages. Its two edited consumer files match the applied files
byte-for-byte. No local path overrides, dependency Git pin changes or lockfile
changes were applied to BuildrootImages. Until shared source is committed and
its Git dependency updated, consumer builds need the local override. Everything
remains uncommitted. Formatting and diff checks passed in both repositories.
No hardware was flashed or physically qualified.


## Polis package naming

Validated on 2026-10-11 with Rust 1.99.0 after renaming all sixteen packages to
`polis-*` and their Rust crate imports to `polis_*`. Package directories, wire
fixtures, transport features and external dependency lockfile entries are
unchanged. Earlier sections below retain the package names and commands used
at the time; use the current READMEs for commands with the new names.

| Check | Result |
| --- | --- |
| `python3 tools/check.py test` | All 32 native package/feature configurations passed, including TCP/UDP and serial tests |
| `python3 tools/check.py build --platform all` and `python3 tools/check.py clippy --platform all` | Native, RP2040 and RP2350 matrices passed; registered embedded examples linked and local Clippy warnings were denied |
| `RUSTDOCFLAGS='-D warnings' python3 tools/check.py doc --platform all` | All supported target/feature configurations documented successfully |
| `python3 tools/check-led-macro.py` | The isolated `led-api` dependency alias caller built on the host and both embedded targets |
| `cargo package --workspace --allow-dirty` | All sixteen archives created and verified with the renamed dependencies |

Archive inspection confirmed normalized dependency names, no remaining internal
dependency paths, and package-local READMEs/provenance. The `polis-framing` archive
verifies with an ordinary compatible version requirement; the old `framing`
exact-version workaround is removed. Formatting, diff whitespace and changed
Markdown local links passed review. Source provenance and licenses are retained.
No crates were published and no hardware was flashed or qualified.

## Point-to-point control-link adapters

Validated on 2026-10-11 with Rust 1.99.0. Added control-link-tokio (TCP default,
optional host serial) and control-link-embassy (UART default, optional USB CDC
and TCP). No discovery handoff or RS-485 control transport. Application identity
checks and session trackers compose with independent framed read/write halves.

```sh
python3 tools/check.py test --package control-link-protocol --package control-link-tokio --package control-link-embassy
python3 tools/check.py build --platform all --package control-link-protocol --package control-link-tokio --package control-link-embassy
python3 tools/check.py clippy --platform all --package control-link-protocol --package control-link-tokio --package control-link-embassy
RUSTDOCFLAGS='-D warnings' python3 tools/check.py doc --package control-link-protocol --package control-link-tokio --package control-link-embassy
cargo test --locked --offline --doc -p control-link-tokio -p control-link-embassy
```

Four Tokio and eight Embassy feature combinations passed native tests, builds,
Clippy and API documentation. RP2040/RP2350 library builds and target Clippy passed;
the fixture linking hardware UART, USB CDC and embassy-net TCP linked on both.
README examples compile with the default transport selections. Formatting and
diff whitespace checks passed. Local TCP tests ran with socket access outside
the filesystem sandbox; host serial tests use a pseudo-terminal.

Tests cover simultaneous traffic, request correlation through a coalesced
identity exchange, events during pending requests, small writes, receive
cancellation with partial input, interrupted sends, zero writes, flush failure,
wrong application, EOF/truncation and USB packet boundaries/ZLP/disconnect.
The framing decoder and unread chunk survive the handshake. Interrupted sends
retire their sender; no requests are automatically replayed.

Package content listing includes package-local licenses, provenance, READMEs,
tests, build script and embedded memory maps. Full registry archive verification
was not repeated; the previously recorded framing registry-name issue remains.
No hardware was flashed, no physical UART/USB/Wi-Fi behavior was qualified, and
neither downstream product was migrated. Optional tokio-serial/embassy-usb
dependencies now belong to control-link, not discovery.

## Discovery cleanup: UDP and RS-485 only

Validated on 2026-10-11 with Rust 1.99.0. Removed point-to-point UART/USB serial
discovery, its role loops, tests, port/CDC dependencies and fixture setup. The
RS-485 scheduler keeps its framed I/O as a private implementation detail. UDP
and RS-485 discovery remain; control-link connection/session work is deferred
to the next implementation step. Tokio defaults to UDP; Embassy has no default
transport. This replaces the earlier UART/serial discovery validation records.

```sh
python3 tools/check.py test --package discovery-protocol --package discovery-tokio --package discovery-embassy
python3 tools/check.py build --platform all --package discovery-protocol --package discovery-tokio --package discovery-embassy
python3 tools/check.py clippy --platform all --package discovery-protocol --package discovery-tokio --package discovery-embassy
RUSTDOCFLAGS='-D warnings' python3 tools/check.py doc --package discovery-protocol --package discovery-tokio --package discovery-embassy
cargo fmt --all -- --check
```

All four transport feature combinations (none, UDP, RS-485, both) passed native
tests, builds, Clippy and API docs. Embedded libraries built on RP2040 and RP2350;
both `udp_link_check` and `rs485_link_check` linked on each target, and target
Clippy passed. UDP tests required local socket access. At this cleanup step the lockfile no longer included embassy-usb or tokio-serial;
they were subsequently added for the control-link adapters above.

Retained tests cover wire fixtures, UDP announcements/probes and filtering,
RS-485 response slots, early/late timestamp bounds, ACK ordering/suppression,
capacity overflow, stalled sends and participant loops. Physical bus timing,
collisions and network interfaces remain unqualified. Fixtures were built, not
flashed. RS-485 still requires a direction-controlled backend with physical TX
completion, measured timing bounds and cancellation recovery.

## Control-link protocol and package layout

Validated on 2026-10-11 with Rust 1.99.0. `framing` lives at
`crates/protocol-framing`; `discovery` was renamed to `discovery-protocol` under
`crates/discovery/discovery-protocol`. The new portable core lives at
`crates/control-link/control-link-protocol`. Discovery's wire schema is unchanged.

The full workspace test suite passed. The control core adds ten unit,
integration and documentation tests, including mixed discovery/control frames
on one stream, keepalives during outstanding requests, cancellation, stale
sessions, duplicate requests, buffer limits and request ID exhaustion.

```sh
cargo test --workspace --locked --offline
python3 tools/check.py build --platform all --package control-link-protocol --package discovery-protocol
python3 tools/check.py clippy --platform all --package control-link-protocol --package discovery-protocol
RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline -p control-link-protocol -p discovery-protocol --no-deps
cargo fmt --all -- --check
```

These checks passed, including actual native, RP2040 and RP2350 library builds.
Runtime adapters, linked runtime examples, consumer migration and hardware tests
are subsequent steps; the protocol core itself performs no I/O.

Archive generation included manifests, source, tests, documentation and licenses.
The standard offline `cargo package` verification selected an unrelated cached
registry `framing 0.1.1` for control-link's version requirement and failed on an
uncached dependency. An online retry failed on registry DNS access. Registry
package naming/resolution must be settled before release.

As a separate archive-content check, all three generated archives were extracted
into `/tmp/polis-control-archives-o2xd8sti` with sibling `[patch.crates-io]`
overrides. `cargo test --workspace --locked --offline` there passed all 36 tests
using only archived source. This verifies archive contents with local dependency
overrides, not an unmodified registry release.

## Initial workspace

Validated on 2026-10-11 with Rust 1.99.0, after adopting bare driver package names.

## Repository checks

| Check | Result |
| --- | --- |
| `python3 tools/check.py test` | 70 unit, integration and documentation tests passed |
| `python3 tools/check.py build --platform all` | All ten libraries built individually on native, RP2040 and RP2350; WS2813 example linked on both MCUs |
| `python3 tools/check.py clippy --platform all` | Passed with warnings denied for local packages |
| `RUSTDOCFLAGS='-D warnings' python3 tools/check.py doc --platform all` | API documentation built for all three configurations |
| `cargo package --workspace` | All ten archives created and verified; dependency paths normalized and source notices/provenance included |

Formatting and local Markdown links also passed review. Archive inspection checked
that each package contains its README/provenance and that normalized dependencies
do not retain local workspace paths. Archives with declared licenses contain the
corresponding notice files.

The offline workspace packaging attempt encountered a Cargo temporary-registry
checksum error. Normal registry-enabled packaging passed. Cargo also reports a
future-compatibility warning in the upstream `proc-macro-error2` dependency; this
did not fail the current compiler/build matrix.

## Remaining release and adoption work

No hardware was flashed or electrically qualified. Discovery provides the shared
wire contract and application filtering, not live socket/session implementations.
Existing discovery consumers still need to adopt the common schema explicitly.

Original packages now use the owner-selected MIT license. Existing MIT/BSD
notices remain on the imported PIO packages. Establish the actual repository URL
and registry ownership
before publication; resolve name conflicts at that time. Remote CI is configured
but has not run because this directory has no established Git remote.

## LED macro validation

After adding `led_outputs!`, the focused package suite passed eight unit tests
and four doctests, including three compile-fail cases. It covers evaluation once
in argument order, caller-name hygiene, mixed lane lengths, eight lanes,
concurrent completion, bad frame lengths and cancellation.

```sh
cargo test --locked -p led-output
python3 tools/check-led-macro.py
python3 tools/check.py build --platform all --package led-output --package pio-sk6812 --package pio-ws2813
python3 tools/check.py clippy --platform all --package led-output --package pio-sk6812 --package pio-ws2813
cargo fmt --all -- --check
```

All commands passed. The isolated caller imports the dependency as `led-api`,
uses borrowed, const-generic lanes without `std`, and builds on the host and both
MCUs. The WS2813 example now uses the macro and links on both MCUs.

## ADS1x1x async validation

Validated on 2026-10-11 with Rust 1.99.0. The workspace now has eleven libraries.
After removing the blocking API, `ads1x1x` has 12 unit tests and two doctests (including a
compile-fail check for selecting a 12-bit rate on a 16-bit model). The complete
workspace suite passes 90 tests.

| Check | Result |
| --- | --- |
| `cargo test --workspace --locked --offline` | 90 unit, integration and documentation tests passed |
| `python3 tools/check.py build --platform all --package ads1x1x` | Native, RP2040 and RP2350 library builds passed |
| `python3 tools/check.py clippy --platform all --package ads1x1x` | All three configurations passed with warnings denied |
| `RUSTDOCFLAGS='-D warnings' cargo doc -p ads1x1x --no-deps --locked --offline` | Passed; README examples are also compiled by doctests |
| `cargo package -p ads1x1x --allow-dirty --offline` | Standalone archive created and verified; source, tests, README, changelog and provenance included |

Wire fixtures exercise yielding async I²C transactions:
all six models, four addresses, signed 12/16-bit samples, both rate tables,
thresholds, ready-output setup, model capability rejection, trigger/MUX writes,
bounded polling, transport failures and reconfiguration after cancellation.
Formatting also passes `cargo fmt --all -- --check`.

An isolated consumer at `/tmp/ads1x1x-validation` linked release ELF images for
both MCUs using Embassy RP 0.10.0, Embassy Time 0.5.1 and Executor 0.10.0. It
instantiates all six models with an actual async Embassy I²C bus,
configures them, performs bounded reads and scales samples. Its memory maps and
linker setup follow the existing WS2813 link fixture. Reproduction in this
session:

```sh
cargo build --offline --locked --manifest-path /tmp/ads1x1x-validation/Cargo.toml --release --target thumbv6m-none-eabi --features rp2040
cargo build --offline --locked --manifest-path /tmp/ads1x1x-validation/Cargo.toml --release --target thumbv8m.main-none-eabihf --features rp235xa
```

The temporary consumer is a link check, not a product migration or hardware
qualification. No ADC was exercised electrically. MIT was subsequently selected
by the owner. Public repository metadata is still needed before publication;
no registry upload was attempted.

## Minimal Embassy driver examples

Validated on 2026-10-11 with Rust 1.99.0. All eight drivers now include
`examples/embassy.rs`, with package-local runtime features and Pico/Pico 2
linker inputs. Source usage demos were simplified; no hardware was flashed.

| Check | Result |
| --- | --- |
| `python3 tools/check.py build --platform all` restricted to the eight driver packages | Native and both MCU library builds passed; all eight Embassy examples linked on both MCUs, plus the existing WS2813 `link_check` |
| `python3 tools/check.py clippy --platform all` with the same package selection | Libraries and embedded examples passed with warnings denied |
| `cargo test --workspace --locked` | 90 unit, integration, and documentation tests passed |
| `cargo fmt --all -- --check` and `git diff --check` | Passed |
| `cargo package -p <driver> --list --allow-dirty --locked` for each driver | All eight package file lists include the usage example, build script, both memory maps, README, and provenance |

The matrix selection used repeated `--package` arguments for `ads1x1x`,
`tmp1x2`, `rs485`, `w5500`, `pio-i2c`, `pio-uart`, `pio-sk6812`, and
`pio-ws2813`. Individual example commands and wiring are in each README.
Archive checks here inspect contents; they do not claim standalone archive
build verification. The existing upstream `proc-macro-error2` future-compatibility
notice remains. Physical sensor readings, serial timing, LED waveforms, and
Ethernet traffic still require bench validation.
