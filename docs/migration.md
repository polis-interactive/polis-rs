# Adding and adopting shared libraries

Package READMEs describe supported APIs and current limitations;
`PROVENANCE.md` files record origins. See [development](development.md) for
reproducible checks and [publishing](publishing.md) for archive verification.

## For each new library

1. Identify the reusable boundary and supported devices. Keep product protocols,
   firmware, BSPs and application policy in their consumer repositories.
2. Record the source revision, local differences, file hashes and license terms.
   Add the implementation, tests, package README and changelog together.
3. Validate host behavior, supported target builds, relevant linked consumers and
   the standalone package archive. Document outstanding hardware checks in the
   package README; report check results in the PR or commit description.

## Consumer adoption

All nineteen workspace libraries now use `polis-<purpose>` Cargo package names.
Update dependency keys and Rust imports together: `framing` becomes
`polis-framing` in Cargo.toml and `polis_framing` in Rust; the same prefix applies
to every other library. Package directories, public items, transport feature
names, and wire formats are unchanged. Update Cargo `-p` / `--package` selections
and dependency feature forwarding to use the new package names too.

For a staged consumer migration, an explicit dependency alias can preserve old
imports while selecting the renamed package:

```toml
[dependencies]
framing = { package = "polis-framing", git = "<polis-rs repository URL>", rev = "<full commit SHA>" }
```

Switch consumers separately to a pinned upstream revision or published version,
validate their images, then retire local copies. Discovery consumers adopt the
shared schema with their own application ID. Package READMEs document API
migration requirements.

Release prerequisites and dependency ordering live in [publishing](publishing.md);
package boundaries and dependency examples live in [architecture](architecture.md).

## HDMI joystick extraction

`drivers/joystick` preserves the former `hdmi-joystick` API under `polis-joystick`.
The source files were untracked in HDMI, so package provenance records their
hashes separately from the surrounding checkout revision. Until this extraction
has a real committed revision or release, HDMI validates it through documented
local Cargo patches, including one ADS driver identity shared by BSP and adapter.
Do not substitute an older polis-rs commit that lacks the new package.
