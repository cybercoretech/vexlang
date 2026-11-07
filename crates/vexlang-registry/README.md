# vexlang-registry

The package-index client behind the [`vexlang`](https://crates.io/crates/vexlang)
CLI (`vexlang add | install | update | remove | search | info | fetch | package | publish | yank`).

Vex packages are listed in a static Git index,
[cybercoretech/vex-index](https://github.com/cybercoretech/vex-index): a single
`index.json` with the tarball URL, SHA-256 and size of every version. There is no
registry server. This crate provides:

- index loading from `https://`, `file://` or a local path, plus resolve, latest
  (SemVer-like ordering, yanked versions skipped) and search;
- `req`: Cargo-style version requirements (`^`, `~`, `=`, `*`, comparisons);
- `install::install`: two-phase dependency installation (resolve and verify into
  a cache, then apply) with `vex.lock`, lock-preferring range resolution,
  `--locked`/`--offline`/update modes, conflict reports and safe unpacking (no
  path traversal, links or special files);
- reproducible `NAME-VERSION.tar.gz` packaging, plus `publish` and `yank` for an
  index checkout.

It is an implementation detail of `vexlang` and its API may change between
releases. Native targets only: it uses blocking I/O.

MIT licensed. Report security issues to security@cybercoretech.net.
