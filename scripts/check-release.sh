#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

version=$(cargo metadata --locked --no-deps --format-version 1 |
    sed -n 's/.*"name":"vexlang","version":"\([^"]*\)".*/\1/p' | head -n 1)
test -n "$version"
test "$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)" = "$version"
if [ "${1:-}" != "" ]; then
    test "$1" = "v$version" || test "$1" = "$version" || {
        echo "release check: tag $1 does not match vexlang $version" >&2
        exit 1
    }
fi

case "$version" in
    *-alpha.*|*-beta.*|*-rc.*) ;;
    *) echo "release check: $version is not a prerelease version" >&2; exit 1 ;;
esac

grep -F "$version" README.md >/dev/null
grep -F "$version" CHANGELOG.md >/dev/null
# Cargo only installs a pre-release when it is named, so the install
# commands must carry the version being released.
for file in README.md site/index.html; do
    grep -F "cargo install vexlang@$version" "$file" >/dev/null || {
        echo "release check: $file lacks \`cargo install vexlang@$version\`" >&2
        exit 1
    }
done
cargo package --locked --allow-dirty --list >/dev/null
echo "release metadata and package contents are consistent for vexlang $version"
