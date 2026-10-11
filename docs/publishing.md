# Publishing libraries

Every reusable package should be independently publishable. Repository location
does not determine release grouping: drivers and software libraries follow the
same release requirements.

## Package contract

| Area | Requirement |
| --- | --- |
| Identity | Available package name, independent SemVer version, description, real repository URL, README, tested `rust-version` |
| Licensing | Correct SPDX expression or license file, applicable notices, retained upstream attribution |
| Dependencies | Registry versions for normal/build dependencies; internal edges use `path` plus `version` |
| Distribution | Package-local source, tests, examples, docs and required build inputs; no dependency on sibling product checkouts |
| Compatibility | Public API/feature policy, changelog, supported targets, documented wire schema when relevant |

Use `publish = ["crates-io"]` for packages ready for public distribution.
An intentional private exception uses package-local `publish = false` and a
documented reason; published libraries must not depend on that private package.
Avoid inheriting Slate's blanket prohibition on publishing.

Each package owns its README and release notes. Shared metadata may be inherited
from the root, but package archives must work outside the monorepo. Inspect
`include_str!`, build scripts, license paths and README links during packaging.
Use docs.rs target/feature metadata for hardware packages; never ask docs.rs to
enable mutually exclusive MCU features together.

## License decisions

The inspected framing manifests have no license, and wired-control's provenance
explicitly preserves that absence. Slate's SK6812/WS2813 derivatives carry MIT
notices; its PIO I2C derivative carries BSD-3-Clause. Preserve these existing
terms and notices on extraction.

The owner must choose public licensing for currently unlicensed original code
before publication. A dual MIT OR Apache-2.0 policy is a proposal for original
contributions, not a license grant made by this scaffold. Package license
expressions must reflect the actual incorporated code; do not mechanically
replace existing upstream terms with a repository default.

## Versioning

Release packages independently, for example `framing-v0.1.0` and
`rs485-v0.1.0`. A framing fix should not require releasing every driver.
Update internal version requirements when a dependent starts using a newly
introduced API. Treat breaking pre-1.0 changes as minor-version bumps.

Track Rust API SemVer, framing format version and product wire schema separately.
A crate rename or dependency migration must preserve existing wire bytes. Changes
to protocol IDs, payload layout or discovery endpoints require an explicit
compatibility plan, even when the Rust API still compiles.

## Local archive verification

Run `cargo package --workspace` to package and verify this workspace together.
Cargo 1.99 stages the local packages in a temporary registry, so initial archive
verification can include unpublished internal dependencies. This creates local
archives; it does not publish them. Per-package public release still follows the
dependency order below.

The offline form hit a Cargo 1.99 temporary-registry checksum error during initial
validation; the normal command completed verification. Keep registry access
available for this check. Archive verification does not grant a license or
establish ownership of a registry name.

## Release procedure

1. Prepare the changed package's version, changelog, dependency requirements and
   provenance; run its supported target/feature and MSRV matrix.
2. Inspect `cargo package -p <package> --list` for required inputs and notices.
   Run `cargo publish -p <package> --dry-run` with the supported target/features
   where necessary. Review the extracted archive independently of workspace paths.
3. Order releases by dependency: shared contracts before their drivers/adapters.
   Initial dry-runs can be blocked by unpublished internal dependency versions;
   record that blocker, validate locally, and repeat dependent dry-runs once those
   prerequisites are available in the registry. Do not use `--no-verify` to claim
   the dependency graph was validated.
4. Publish the reviewed versions when publication is explicitly requested, then
   create the corresponding package tags/release notes in the established remote.
5. Update downstream products to the released versions and run their host and
   firmware builds before retiring temporary patches or migration copies.

Cargo's [publishing guide](https://doc.rust-lang.org/cargo/reference/publishing.html)
describes archive verification and dry runs. Its
[dependency guide](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#multiple-locations)
explains how `path` plus `version` becomes a registry dependency when published.

Begin with manual per-package releases. Add release automation only after one
complete release establishes the workflow. Repository hosting, registry ownership
and final package-name availability remain inputs to the first public release.
