# Changelog

## Unreleased

- Fixed REPL evaluation so each accepted input runs once instead of replaying
  earlier statements and repeating their side effects. Runtime state now rolls
  back with failed snippets, with regression coverage for mutable bindings,
  function persistence, and side-effect counts.
- Added a structured IR verifier (`IR001` invariant failures and `IR002`
  deferred-feature failures) that runs before `build`/`ir` artifacts are
  rendered.
- Documented the native lowering subset and runtime/backend contract.
- Added the first native target contract (`vex-scalar-v1`), target validator,
  `TextBackend` emitter, and `target` CLI command.
- Added a backend trait boundary and an experimental `QbeBackend`/`qbe` CLI
  command for the verified scalar subset without producing fake native
  executable artifacts.
- Hardened experimental QBE scalar runtime semantics with distinct generated
  traps for i32 overflow (`101`), division by zero (`102`), and signed division
  overflow (`103`).
- Expanded compiled-vs-interpreted QBE conformance coverage for scalar
  arithmetic, control flow, functions, booleans, and runtime traps.
- Added reusable QBE install/smoke scripts, QBE backend documentation, and
  `u64` target design notes.
- Added `--json` / `--diagnostic-format=json` machine-readable source
  diagnostics for CLI tooling, with label support for secondary source ranges.
- Started AST span retention by preserving top-level statement spans during AST
  construction.
- Added conservative multi-error recovery for undefined-variable and
  undefined-function diagnostics in `check`, including JSON-array output when
  multiple diagnostics are emitted.
- Added declaration-site labels for selected semantic diagnostics such as type
  mismatches.
- Added parser-collected expression spans, analyzer-native multi-error
  collection for undefined names, and conservative line-oriented parse recovery
  for `check`.

## 0.1.0-alpha.1

- Hardened public-alpha release metadata with `vexlang --version`,
  package consistency checks, reproducible source artifacts, and checksums.
- Added deterministic textual-IR and release smoke coverage and a documented
  publish checklist. Native binaries are intentionally not produced.

## Unreleased (alpha)

- Added enum/Result `match` expressions, wildcard and binding patterns,
  exhaustiveness diagnostics, and interpreter Result propagation with `?`.
  Match/propagation textual-IR lowering is deferred until sum-value control
  flow is represented in the IR.
- The REPL now preserves successful bindings/functions, supports multiline
  input and inspection/loading commands, and rolls back failed snippets.
- Added typed array collections: `[1, 2, 3]`, `[i32]` annotations, empty and
  nested arrays, zero-based indexing, indexed mutation, bounds/type diagnostics,
  and textual IR lowering. Maps and generics remain deferred.
- Stabilized parsing and AST construction for the core expression language.
- Added precedence-aware arithmetic, comparisons, unary minus, variables, and
  `let` declarations.
- Added evaluator environments, overflow and division-by-zero checks, and
  structured CLI errors.
- The CLI now reads a source file or stdin instead of evaluating hardcoded
  source.
- Earlier incomplete QBE code-generation path was disabled while the
  interpreter was being established.
- Added boolean short-circuiting, typed conditional blocks, `if`/`else`, and
  `while` loops.
- Added integer annotation range checks and removed unsupported declaration
  syntax from the active grammar.
- Added mutable assignment so loops can update program state.
- Added `break` and `continue` with loop-only semantic validation, nested
  control-flow propagation, and a one-million-iteration safety limit.

Vex remains an alpha prototype; syntax and APIs may change.

The planned map mutation and iteration work remains deferred until collection
types share a common representation; non-string keys are not silently added.

## Unreleased

- Added user-defined record declarations (`record Name { field: type, ... }`),
  record construction, validated field access, interpreter support, and
  textual-IR lowering. Collections, modules, structured errors/results, and
  project configuration remain deferred.

- Added typed function declarations, calls, returns, recursion, scoped
  parameters, argument/return checking, and call-depth protection.
- Added distinct `u64` runtime values and checked `i32`/`u64` conversions.
- Added string literals plus `print` and `println` built-ins.
- Deferred user-defined types, collections, filesystem access, and modules.
- Added structured diagnostics with stable codes, source underlines,
  line/column locations, and actionable suggestions.
- Added `check`, `run`, `fmt`, `test`, and explicit unsupported `build` CLI
  commands while preserving the legacy file argument form.
- Milestone 8: added typed lowering to a deterministic textual IR exposed by
  `build` and `ir`. Machine-code executable artifacts remain intentionally
  deferred.
- Milestone 9: added interpreter/IR compatibility coverage, stable CLI
  diagnostic and generated-input tests, an opt-in performance smoke test, and
  release-readiness policy documentation.
- Public-release preparation: selected the MIT license, added a private
  security contact, and defined the `0.1.0-alpha.1` release as an alpha
  interpreter and typed-IR prototype.
