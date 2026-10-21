# Vex completed phases

This file records delivered phases. Forward-looking work is tracked in
[`README_MILESTONES.md`](README_MILESTONES.md).

## Phase 1 — Core language stabilization

- Established the interpreter pipeline and authoritative grammar.
- Added arithmetic, comparisons, booleans, variables, declarations, inference,
  assignment, blocks, and scoped evaluation.
- Added overflow, division-by-zero, range, and type validation.

## Phase 2 — Control flow

- Added `if`/`else`, `while`, `break`, and `continue`.
- Added nested control-flow propagation and a one-million-iteration loop limit.

## Phase 3 — Functions and types

- Added typed functions, parameters, returns, calls, recursion, and
  call-depth protection.
- Added distinct `i32`/`u64` runtime values and checked conversions.

## Phase 4 — Standard library foundation

- Added strings, `print`, and `println`.
- Documented deferred collections, filesystem access, modules, and resources.

## Phase 5 — Diagnostics and CLI tooling

- Added stable diagnostic codes, source locations, underlines, and suggestions.
- Added `check`, `run`, `fmt`, `test`, `build`, and `ir` commands.
- Defined CLI exit statuses and preserved the legacy file invocation.

## Phase 6 — Typed IR

- Added backend-independent typed lowering.
- Added deterministic textual IR.
- Added interpreter/lowering consistency tests.
- Added explicit IR verification with structured invariant and unsupported
  feature errors before any backend boundary.
- Documented the lowerable subset and the runtime contract required by a first
  native target.
- Added the `vex-scalar-v1` target contract, target validation, and the `target`
  CLI command emitting verified target IR.
- Kept native executable output explicitly unsupported until a verified backend
  and runtime artifact story exist.

## Phase 7 — Compatibility and release readiness

- Added compatibility corpus, golden diagnostics, generated-input tests, and
  an opt-in performance smoke test.
- Added MIT licensing, security policy, MSRV metadata, CI quality gates, and
  complete mdBook documentation.
- Published the project as `0.1.0-alpha.1`, an alpha interpreter and typed-IR
  prototype.

## Phase 8 — Public alpha hardening

- Added `--version` and release-readiness checks tying the CLI, Cargo metadata,
  README, and changelog to the same prerelease version.
- Added reproducible Cargo source-package and SHA-256 artifact scripts without
  inventing native binaries.
- Added deterministic textual-IR and version smoke coverage, plus tag-triggered
  package checks in CI.
- Documented the public-alpha publish checklist and exact production-release
  blockers.

## Current release posture

The repository is public-ready for the labeled alpha scope. It is not yet a
production compiler release: native executable generation, map mutation and
iteration, generics, modules, and project/package management remain future work.

## Phase 15 — Pattern matching and Result propagation

- Added enum and `Result` match expressions with wildcard and binding patterns.
- Added semantic exhaustiveness checks and typed pattern scopes.
- Added interpreter Result propagation with `?`.
- Textual IR lowering for these expressions remains deferred until the IR has
  first-class sum-value control flow.

## Phase 16 — Stateful REPL

- REPL bindings and function declarations persist across successful inputs.
- Added multiline brace-aware input and `:help`, `:reset`, `:type`, `:ir`,
  `:load`, and `:quit` commands.
- Each input executes once, so prior side effects are not replayed.
- Failed snippets roll back both the input history and runtime session state.

## Phase 17 — Local modules and projects

- Deferred: the existing `project` reader remains metadata-only; imports,
  module graph resolution, project entrypoint selection, and project
  subcommands require a larger manifest and filesystem design.

## Phase 18 — Mutable maps and iteration

- Deferred: maps remain string-keyed and immutable through the public
  expression API; mutation and iteration need a shared collection abstraction.

## Phase 19 — Type-system foundations

- Deferred: aliases, tuples, generic parameters, and conversions need a
  shared type representation before they can be added safely.

## Phase 20 — Compiler readiness

- Deferred: AST-wide spans, structured diagnostics, broader IR validation, and
  backend prerequisites remain ahead of native executable generation. No fake
  native output is emitted.

## Phase 21 — Experimental scalar QBE IL

- Added a `QbeBackend` for the verified `vex-scalar-v1` subset.
- Added `vexlang qbe` to emit QBE IL for scalar functions, calls, local slots,
  branches, loops, `break`/`continue`, returns, and short-circuit boolean logic.
- Added structural backend coverage plus an optional compiled-vs-interpreted
  differential smoke test that runs only when local `qbe` and `cc` tools exist.
- Native executable packaging, runtime ABI expansion, and aggregate lowering
  remain deferred.

## Phase 9 — Typed collections and indexing

- Added typed array literals, including empty and nested arrays.
- Added analyzer checks for element types, integer indices, and indexed
  assignment.
- Added evaluator indexing, nested mutation, and bounds diagnostics.
- Added typed textual-IR array and index lowering.
- Maps and generics are explicitly deferred.

## Phase 10 — Enums and sum values

- Added enum declarations with unit and named-field variants.
- Added typed enum construction, semantic validation, interpreter values, and
  textual IR rendering.
- Pattern matching and exhaustive checking remain deferred.

## Phase 11 — Structured result values

- Added `ok(value)` and `err(value)` constructors.
- Added typed semantic checking, runtime result values, display formatting,
  and IR builtin lowering.
- Result propagation, typed error parameters, and matching remain deferred.

## Phase 12 — Interactive REPL

- Added the `vexlang repl` command with prompts, line evaluation, and `:quit`
  / `:q` exit commands.
- The REPL intentionally evaluates each line independently; persistent
  bindings and multiline editing remain deferred.

## Phase 13 — Project configuration foundations

- Added a dependency-free `vex.toml` project metadata reader and `project`
  CLI command.
- Module resolution, imports, manifests beyond the `[project]` section, and
  dependency management remain deferred.

## Phase 14 — Scoped maps

- Added a string-keyed map collection with `map` construction and `map_get`.
- Added analyzer validation, interpreter values, display formatting, and IR
  builtin typing.
- Full generics, map mutation/iteration, and non-string keys are deferred.
