# Adding and adopting shared libraries

The initial library extraction is complete. Package READMEs describe supported
APIs; `PROVENANCE.md` files record origins and `validation.md` records checks.
The ADS1x1x driver now covers the six related I²C models from the UTSuites ADS1015
reference. Further drivers can follow the same process.

## For each new library

1. Identify the reusable boundary and supported devices. Keep product protocols,
   firmware, BSPs and application policy in their consumer repositories.
2. Record the source revision, local differences, file hashes and license terms.
   Add the implementation, tests, package README and changelog together.
3. Validate host behavior, supported target builds, relevant linked consumers and
   the standalone package archive. Record hardware checks still outstanding.

## Consumer adoption

Switch consumers separately to a pinned upstream revision or published version,
validate their images, then retire local copies. Discovery consumers adopt the
shared schema with their own application ID; UTSuites-specific control protocols
remain downstream. ADS1015 consumers need the API changes in the `ads1x1x` README.

Release prerequisites and dependency ordering live in [publishing](publishing.md);
package boundaries and dependency examples live in [architecture](architecture.md).
