# ⚡ Vex

> **A cybercore language lab for safe experiments in syntax, semantics, and
> typed lowering.**

Vex is an **alpha interpreter and typed-IR prototype** for a deliberately small
language. It is not a production compiler or native toolchain yet. Vex is
released under the [MIT license](LICENSE).

[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![CI](https://github.com/cybercoretech/vexlang/actions/workflows/quality.yml/badge.svg)](https://github.com/cybercoretech/vexlang/actions/workflows/quality.yml)
[![Release](https://img.shields.io/badge/release-0.1.0--alpha.1-purple)](https://github.com/cybercoretech/vexlang)

## 🛰️ Signal status

```text
RUNTIME   [ONLINE]   interpreter + typed textual IR
SAFETY    [GREEN]    text/JSON diagnostics, limits, MSRV, CI gates
BACKEND   [EXPERIMENTAL] verified scalar QBE IL; no native executable artifacts
CHANNEL   [ALPHA]   syntax and APIs may evolve
```

## 🧬 Supported language

```vex
let base: i32 = 10;
let answer = -(base + 2) * 3;
answer;
```

The interpreter supports integer and boolean literals, `let` declarations,
variables, `+`, `-`, `*`, `/`, `==`, `<`, `>`, `&&`, `||`, unary minus, logical
not, `if`/`else`, `while`, `break`, `continue`, blocks, and expression
statements. Blocks may have local declarations and a final expression.

Variables can be updated with assignment:

```vex
let x = 0;
while x < 3 { x = x + 1; }
x;
```

`break` exits the innermost loop and `continue` starts its next iteration; both
are rejected outside loops. Loops stop with a runtime error after one million
iterations to prevent runaway programs.

The `i32`, `u64`, and `bool` annotations are supported. Functions support typed
parameters, returns, recursion, and bounded call depth. User-defined records
support declarations, construction, validated fields, and field access:

```vex
record Point { x: i32, y: i32 }
let p: Point = Point { x: 2, y: 3 };
p.x + p.y;
```

Strings and `print`/`println` are available in the standard library. Arrays
support literals, nesting, typed annotations such as `[i32]`, zero-based
indexing, and indexed assignment:

```vex
let values: [i32] = [1, 2, 3];
values[1] = 9;
values[1];
```

The interpreter also supports enums, `ok(value)`/`err(value)` results,
exhaustive `match` expressions, `?` propagation, and small string-keyed maps
with `map` and `map_get`. The CLI includes a stateful REPL and a
metadata-only `vex.toml` project reader. Generics, map mutation/iteration,
modules/imports, filesystem access, floating point, `res`, and packaged
native executable generation remain deferred.

## 🧪 Quick start

Run or validate a source file:

```sh
cargo run -- run program.vex
cargo run -- check program.vex
cargo run -- test program.vex
cargo run -- build program.vex
cargo run -- ir program.vex
cargo run -- target program.vex
cargo run -- qbe program.vex
cargo run -- repl
cargo run -- project vex.toml
cargo run -- --json check program.vex
```

`fmt` validates source and writes it to stdout. `build` and `ir` validate and
lower the program to deterministic textual IR; they do not emit machine code.
`target` validates the IR against the `vex-scalar-v1` contract and emits
a verified textual target artifact. `qbe` emits experimental QBE IL for the
same scalar subset; it is an intermediate artifact, not a packaged native
executable.
Before rendering, the IR verifier checks scopes, types, declarations, control
flow, and backend support. A failed verification is a hard error: no target
artifact is produced.
The legacy `cargo run -- program.vex` form remains an alias for `run`.

Read source from stdin:

```sh
printf 'let x = 6; x * 7;' | cargo run
printf '1 + 2;' | cargo run -- -
```

Diagnostics include stable error codes, file/line/column locations, source
underlines, and suggestions. Use `--json` or `--diagnostic-format=json` before
a command to emit one machine-readable diagnostic object on stderr. Exit status
`0` means success, `1` means a source or test failure, `2` means a CLI or I/O
error, and `3` means an explicitly unsupported target/backend operation.

### Usage warnings

- Vex is still alpha software; syntax, IR details, and experimental backend
  behavior may change before a production release.
- `qbe` emits QBE IL only. It does **not** assemble, link, package, or publish a
  native executable.
- The QBE backend currently accepts only the verified scalar subset: `i32`,
  `bool`, `unit`, declared function calls, locals, structured control flow,
  loop control, returns, and checked scalar arithmetic.
- Interpreter features such as strings, `u64`, records, arrays, maps, enums,
  `Result`, and I/O builtins are rejected at the `vex-scalar-v1` target
  boundary until their runtime ABI is specified.
- If QBE output is assembled and run manually, runtime traps exit with `101`
  for checked `i32` overflow, `102` for division by zero, and `103` for signed
  division overflow.

## 🗺️ Project roadmap

```text
VEX
├── ✅ Phase 1  Core language stabilization
├── ✅ Phase 2  Control flow
├── ✅ Phase 3  Functions and types
├── ✅ Phase 4  Standard library foundation
├── ✅ Phase 5  Diagnostics and CLI tooling
├── ✅ Phase 6  Typed, backend-independent IR
├── ✅ Phase 7  Compatibility and release readiness
├── 🚧 Phase 8  Public alpha hardening
│   ├── release artifacts and version checks
│   ├── stronger fuzzing and benchmarks
│   └── stable + MSRV quality gates
├── 🚧 Phase 9  Language expansion
│   ├── ✅ user-defined record types
│   ├── ✅ typed arrays and indexing
│   ├── ✅ interpreter enums, Result propagation, and scoped maps
│   └── ⏳ modules, generics, and project/package management
├── 🚧 Milestone 12 Production diagnostics
│   ├── ✅ initial machine-readable JSON diagnostics
│   ├── AST-wide source spans
│   ├── multi-span diagnostics
│   └── multi-error recovery
├── 🚧 Milestone 13 Native compilation
│   ├── ✅ experimental scalar QBE IL backend
│   ├── ✅ compiled/interpreted QBE conformance smoke tests
│   ├── target expansion, starting with `u64` design
│   └── reproducible native executable artifacts
└── 🔭 Milestone 14 Ecosystem and tooling
    ├── package management
    ├── LSP/editor integration
    └── formatter and compatibility policies
```

See the [future milestone plan](README_MILESTONES.md) and
[completed phases](README_COMPLETED_PHASES.md) for detailed scope and status.

Try the themed [Vex Language Lab in DarkwaveCode](https://darkwavecode.pages.dev/vexlab.html).
It provides guided examples and an editable Workbench starter; execution still
requires the Vex CLI.

## 🛠️ Development

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo +1.85.0 test --all-targets
cargo +1.85.0 clippy --all-targets -- -D warnings
mdbook test docs
./scripts/qbe-smoke.sh
```

The QBE smoke script requires `qbe` and `cc` on `PATH`; use
`PREFIX="$HOME/.local" ./scripts/install-qbe.sh` to install QBE locally. The
release-readiness suite includes compatibility, golden diagnostic, JSON
diagnostic, QBE fixture, and generated-input tests, plus an opt-in performance
smoke test:

```sh
cargo test --test release_readiness -- --ignored --nocapture
```

The minimum supported Rust version (MSRV) is **1.85.0**, matching Rust 2024.
The complete [Vex mdBook documentation](docs/src/intro.md) includes the
[language reference](docs/src/language.md), [CLI and exit codes](docs/src/cli.md),
[diagnostic format](docs/src/diagnostics.md), [feature status](docs/src/features.md),
[development gates](docs/src/development.md), [experimental QBE backend](docs/src/qbe.md),
[`u64` target design notes](docs/src/u64-target-design.md), and [roadmap](docs/src/milestones.md).

## 📦 Public alpha release checklist

Current release: `0.1.0-alpha.1`.

The package version is declared in `Cargo.toml` and exposed by
`vexlang --version`. Before publishing a tagged alpha, run:

```sh
./scripts/check-release.sh
./scripts/release-artifacts.sh dist
```

The second command produces the Cargo source package and a SHA-256 checksum
under `dist/`. The package contains source for the interpreter, textual IR, and
experimental QBE IL emitter; it does not contain a native executable. Verify
the checksum and attach both files to the matching Git tag. The complete
publish checklist is in the [release guide](docs/src/release.md).

## 🤝 Contributing

Small, focused changes are welcome. See
[CONTRIBUTING.md](CONTRIBUTING.md) for setup, good-first-contribution ideas,
language-change expectations, pull-request guidance, and all quality gates.
The current Discord contact is **`darkstar_dev`**; a dedicated server invite
will be added when the community space is established. Please review the
[security policy](SECURITY.md) before reporting vulnerabilities.

QBE output is available through `vexlang qbe` for the first scalar target. It
remains experimental and intentionally stops at QBE IL; release artifacts still
contain source packages rather than native executables.

### Native lowering contract

The first native target consumes only verified IR containing `i32`, `bool`,
`unit`, calls to declared functions, and structured `if`/`while`/loop-control.
The contract documents the calling convention, checked integer overflow and
division traps, control-flow model, runtime boundary, and `vex_main` entry
point. Experimental QBE trap exits are `101` for checked `i32` overflow, `102`
for division by zero, and `103` for signed division overflow.
Strings, `u64`, records, arrays, field projection, indexed mutation, enums,
`Result`, maps, and I/O builtins are currently deferred because no runtime ABI
has been specified. The `Backend` trait is the boundary used by both the
textual target artifact and the experimental QBE IL emitter.
