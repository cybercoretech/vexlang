#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
output=${1:-"$root/dist"}
cd "$root"

version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
test -n "$version"
rm -rf "$output"
mkdir -p "$output"
# Package the whole workspace in one go: vexlang depends on vexlang-registry,
# and `--workspace` lets Cargo resolve it from the local package before it
# exists on crates.io.
cargo package --workspace --locked --allow-dirty --no-verify
target_dir=${CARGO_TARGET_DIR:-$(cargo metadata --locked --no-deps --format-version 1 |
    sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p' | head -n 1)}
archive="$target_dir/package/vexlang-$version.crate"
test -f "$archive"
cp "$archive" "$output/"
(cd "$output" && sha256sum "vexlang-$version.crate" > "vexlang-$version.crate.sha256")
registry_version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' crates/vexlang-registry/Cargo.toml | head -n 1)
registry="$target_dir/package/vexlang-registry-$registry_version.crate"
test -f "$registry"
cp "$registry" "$output/"
(cd "$output" && sha256sum "vexlang-registry-$registry_version.crate" > "vexlang-registry-$registry_version.crate.sha256")
printf '%s\n' "wrote $output/vexlang-$version.crate, $output/vexlang-registry-$registry_version.crate and their SHA-256 checksums"
