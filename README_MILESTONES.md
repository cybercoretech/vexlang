# Vex milestones

This file is the forward-looking project plan. Completed work is recorded
separately in [`README_COMPLETED_PHASES.md`](README_COMPLETED_PHASES.md).

## Milestone 8 — Public alpha hardening

- Keep the `0.1.0-alpha.1` syntax and command behavior stable.
- Check package/CLI/documentation version consistency.
- Produce a reproducible Cargo source artifact and SHA-256 checksum.
- Exercise generated inputs, deterministic textual IR, compatibility, and
  performance without heavyweight dependencies.
- Keep stable, MSRV, documentation, and release-package CI gates green.

**Current status:** Complete for the public alpha scope. A production `0.1`
release remains blocked by packaged native executable generation, broader
backend validation, and deferred language features.

## Milestone 9 — Language expansion

- Add user-defined types and records. **Records are complete:** declarations,
  construction, field access, semantic field/type validation, interpreter
  evaluation, and textual-IR lowering are covered by unit and CLI tests.
- Add typed array collections and indexing. **Complete:** array literals,
  nested arrays, annotations, indexing, mutation, diagnostics, and textual IR.
- Maps and generics remain explicitly deferred.
- Add a structured error/result model.
- Define modules, imports, and project configuration.

Phases 15 and 16 (interpreter pattern matching/Result propagation and the
stateful REPL) are complete. Scoped maps are implemented; mutation and iteration
remain deferred. The forward sequence is local modules/projects, map mutation
and iteration, type aliases/tuples/generic foundations, and compiler-readiness
validation. Experimental QBE IL exists for the first scalar subset; packaged
native executable generation remains out of scope until validation and runtime
ABI coverage are broader.

Phase 17 is currently a documented boundary: local module/import resolution
and project entrypoint behavior are not yet implemented.

The type-system phase explicitly defers aliases, tuples, generic foundations,
and broad conversion changes until they can use one shared representation.

Compiler readiness remains validation-first: fake native artifacts are
prohibited, QBE IL is emitted only for the verified scalar subset, and
spans/diagnostics/IR validation must be expanded before native executable
artifacts are introduced.

## Milestone 12 — Production diagnostics

- Retain source spans on every AST node. **Started:** AST construction now
  preserves top-level statement spans and expression spans from the parse tree;
  semantic diagnostics can attach declaration-site labels. Full typed AST span
  attachment remains in progress.
- Add richer multi-span diagnostics and machine-readable output. **Initial
  machine-readable JSON diagnostics are available through `--json`; diagnostic
  labels are represented in text and JSON, while full AST-wide multi-span output
  remains in progress.**
- Add error recovery for multiple diagnostics per invocation. **Started:**
  `check` uses analyzer-native collection for multiple undefined-variable and
  undefined-function diagnostics, plus conservative line-oriented parse
  recovery.

## Milestone 13 — Native compilation

- Harden the experimental QBE backend behind the existing typed IR.
- Expand compiled-vs-interpreted differential tests beyond the optional local
  QBE/`cc` smoke corpus.
- Define target support and reproducible native executable build artifacts.

## Milestone 14 — Ecosystem and tooling

- Add package/project management.
- Add editor/LSP integration.
- Add formatter guarantees and language-server diagnostics.
- Publish stable standard-library and compatibility policies.

## Quality gate

Every milestone must preserve:

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo +1.85.0 test --all-targets
cargo +1.85.0 clippy --all-targets -- -D warnings
mdbook test docs
```
