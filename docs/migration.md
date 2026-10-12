# Adding and adopting shared libraries

The initial library extraction is complete. Package READMEs describe supported
APIs; `PROVENANCE.md` files record origins and `validation.md` records checks.
Further libraries can follow the same process.

## For each new library

1. Identify the reusable boundary and supported devices. Keep product protocols,
   firmware, BSPs and application policy in their consumer repositories.
2. Record the source revision, local differences, file hashes and license terms.
   Add the implementation, tests, package README and changelog together.
3. Validate host behavior, supported target builds, relevant linked consumers and
   the standalone package archive. Record hardware checks still outstanding.

## Consumer adoption

All sixteen workspace libraries now use `polis-<purpose>` Cargo package names.
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
