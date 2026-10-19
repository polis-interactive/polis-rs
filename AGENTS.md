# Repository guidance

- This repository contains reusable libraries. Product applications, firmware,
  BSP assemblies, hardware exports, deployment policy, and product configuration
  belong in downstream repositories. Tests and focused examples are allowed.
- Use one agent unless the user explicitly enables delegation. Preserve unrelated
  changes; leave edits unstaged and uncommitted unless requested otherwise.
- All Cargo artifacts must use the repository-root `.target/`, including new
  packages, examples, tests, and package verification. Inherit the workspace
  `.cargo/config.toml`; never create a package-local `target/` or use Cargo's
  default `target/` directory. For commands outside the workspace (including
  extracted archives), explicitly set `CARGO_TARGET_DIR` to the absolute path
  of the owning repository's `.target/` directory.
- Read [architecture](docs/architecture.md) for package boundaries,
  [development](docs/development.md) for Cargo/build changes,
  [publishing](docs/publishing.md) for manifests/releases, and
  [migration](docs/migration.md) before extracting code.
- Activate packages only with real implementations. Keep shared embedded code
  usable without `std` or allocation, isolate host runtimes and MCU features,
  preserve wire compatibility, and retain source provenance and licenses.
- Validate changed libraries with relevant host tests and actual target builds.
  Linked examples or downstream builds validate runtime/linker integration.
  Documentation-only changes need static review. Report blocked validation;
  do not equate `cargo check` with compilation/linking or hardware qualification.
