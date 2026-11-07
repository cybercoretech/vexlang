# Public alpha release guide

Vex `0.1.0-alpha.3` is an interpreter and typed textual-IR prototype with an
experimental scalar QBE IL emitter. A release publishes the Cargo source
package; it does **not** publish a native binary or claim production compiler
support.

Typed arrays and indexing are included in this alpha's interpreter and
textual-IR scope. Maps and generics remain intentionally deferred.

## Publish checklist

1. Confirm the intended version in `Cargo.toml`, then update the matching
   changelog entry and README release badge/text.
2. Run `./scripts/check-release.sh`. It checks Cargo metadata, prerelease
   naming, documentation references, and package inclusion.
3. Run the complete [quality gate](development.md), including Rust 1.85.0 and
   `mdbook test docs`.
4. Run `./scripts/release-artifacts.sh dist`. It writes both `.crate` files
   and their checksums.
5. Verify the checksum from the artifact directory with
   `cd dist && sha256sum --check *.crate.sha256`.
6. Inspect the archive with `tar tf` and confirm it contains source and docs,
   not a fabricated native executable.
7. Dry-run the crates.io upload: `cargo publish --workspace --dry-run`. This
   packages and builds both `vexlang-registry` and `vexlang` from their
   tarballs.
8. Create the matching annotated Git tag, run `cargo publish --workspace`
   (it publishes `vexlang-registry` first, then `vexlang`), and attach the
   `.crate` files and checksums from `dist/` to the GitHub release.
9. Record the tag, artifact checksums, and validation results in the release
   notes.

The tag workflow repeats the metadata check and package smoke test. Artifact
creation uses Cargo's locked dependency resolution and is deterministic for a
given source tree and lockfile.

## Current blockers

A production `0.1` compiler release is blocked by native executable artifacts,
broader backend validation/runtime ABI work, maps, generics, modules, and
project/package management. Arrays and records are supported in the alpha
interpreter and textual IR; the remaining items are deliberate roadmap work,
not alpha release failures.
