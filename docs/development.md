# Workspace development

One active virtual workspace contains eleven library packages. Rust 1.99.0 is the
pinned contributor compiler and the initial supported compiler floor. This is a
conservative tested floor, not a claim that older compilers cannot work. Lower a
package's `rust-version` only after testing it and its resolved dependencies.

## Native development

```sh
cargo test --locked
cargo fmt --all -- --check
python3 tools/check.py clippy
python3 tools/check.py build
```

All current members have a host-compatible configuration with no chip selected.
PIO host builds expose portable types and test timing/waveform code; select an MCU
feature for the hardware API. Each package owns an independent version. Shared
versions and internal `path` plus `version` declarations live in root
`workspace.dependencies`. Keep the root Cargo.lock under version control.

The build tool uses Python 3.11+ and checks packages individually to catch
accidental reliance on another member's enabled features. CI and the editor use
this same tool. CI uses `--locked`; interactive editor checks use `--unlocked`
so a manifest edit can regenerate the lockfile.

## Platform selection

```sh
python3 tools/check.py build --platform all
python3 tools/check.py build --platform rp2040 --package pio-i2c
python3 tools/check.py build --platform rp2350 --package pio-ws2813
```

| Platform | Target | Feature forwarded to PIO drivers |
| --- | --- | --- |
| `native` | Host | None |
| `rp2040` | `thumbv6m-none-eabi` | `rp2040` |
| `rp2350` | `thumbv8m.main-none-eabihf` | `rp235xa` |

The map lives in `[workspace.metadata.platforms]`; package metadata marks
libraries with chip features and linked examples. A virtual workspace has no
package-level build features, so the tool translates `--platform` into ordinary
Cargo arguments. Consumers use the package features directly. Never set a global
embedded target or enable both MCU features in one invocation.

`build` produces actual library artifacts on embedded targets and also links
registered examples. `check` is available for faster iteration but does not
replace builds. `clippy` checks the same feature/target combinations. `test` is
native-only. `doc` builds each library's API documentation and selects hardware
APIs when given an embedded platform.

The WS2813 `link_check` example is opt-in and owns its memory maps and linker
arguments. Its runtime dependencies are enabled by `link-check`; ordinary library
builds do not select an executor, time driver, panic handler, or boot image.
The other hardware drivers need consuming-image checks for concrete integration.
No build command flashes hardware.

## Editor and CI

VS Code formats on save and runs the native matrix through
`.vscode/settings.json`. For an embedded editing session, change the override's
platform and set rust-analyzer's analysis target/features consistently. Rust
Analyzer provides one selected semantic configuration at a time; the separate
matrix invocations validate all supported combinations.

`.github/workflows/ci.yml` defines native tests, Clippy, docs, and builds plus
separate RP2040/RP2350 build and Clippy jobs. The workflow is prepared locally;
remote execution requires hosting this repository. Python build tooling is not a
product application and does not become a downstream library dependency.

## Dependency and release checks

Keep native and embedded libraries allocation-free where that is their contract.
Disable dependency default features at workspace declaration sites when shared
embedded dependencies need it. Optional chip/runtime/logging choices belong to
the package or consumer that requests them, not every workspace member.

The lockfile makes repository checks reproducible. Before a release, also test
fresh dependency resolution and each advertised compiler floor. A published
library's consumers resolve their own dependency graphs and retain their own
profiles, compiler pins and target settings.

See [publishing](publishing.md) for per-package archive checks and registry
prerequisites. Cargo documents [workspace metadata](https://doc.rust-lang.org/cargo/reference/workspaces.html)
and [feature unification](https://doc.rust-lang.org/cargo/reference/features.html).
