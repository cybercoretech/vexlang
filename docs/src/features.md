# Features and limitations

## Supported

The interpreter currently supports:

* integer, boolean, and string literals;
* inferred declarations and `i32`, `u64`, `bool`, and `string` annotations;
* checked arithmetic, comparisons, equality, boolean short-circuiting, and
  explicit integer conversions;
* variables, assignment, blocks, `if`/`else`, `while`, `break`, and `continue`;
* typed functions, returns, recursion, and bounded call depth;
* user-defined record declarations, construction, validated fields, and access;
* typed arrays, nested/empty literals, indexing, mutation, and bounds checks;
* enum values, exhaustive `match`, structured `Result` values, and `?`
  propagation in the interpreter;
* scoped string-keyed maps with `map` and `map_get`;
* `print` and `println`;
* structured diagnostics and the `check`, `run`, `fmt`, `test`, `build`, `ir`,
  `target`, `qbe`, `repl`, and metadata-only `project` commands;
* a typed, deterministic, backend-independent textual IR;
* experimental QBE IL emission for the verified scalar target subset.

## Deferred or intentionally unsupported

Floating point values, resources (`res`), map mutation/iteration, generics,
filesystem access, modules/imports, project execution, and lockfile semantics
are deferred. Project metadata can be read from a small `vex.toml`. Record
types are the first supported user-defined type. Milestone 9's lightweight
compatibility, diagnostic,
generated-input, and opt-in performance smoke tests are now present; a
coverage-guided fuzzing service remains deferred.

Vex has no packaged native executable backend. `build` stops after typed
lowering and emits textual IR; `qbe` emits experimental QBE IL only after the
scalar target validator accepts the program. Floating point, resource, custom
types, and other constructs are rejected explicitly by the lowering or target
validation stage rather than silently approximated. Record lowering is
supported for textual IR; native executable generation is not.

## Lowerable IR contract

`build` and `ir` run a verifier before producing textual IR. That lowerable
textual-IR subset is deliberately broader than the first native target and can
represent `i32`, `u64`, `bool`, `unit`, arithmetic/boolean expressions,
structured control flow, declared functions, record construction, and immutable
arrays. The `vex-scalar-v1` target used by `target` and `qbe` is narrower:
`i32`, `bool`, `unit`, declared function calls, structured branches, loops,
loop control, and returns. Strings, `u64`, record field projection, indexed
mutation, enums, `Result`, maps, and I/O builtins are deferred until their
native representation and runtime ABI are specified. Failures carry a stable
`IR001` (invalid invariant) or `IR002` (unsupported feature) code and a path
into the IR. No target artifact is emitted after a failure.

The backend contract specifies calling convention, entry point, checked scalar
integer overflow/division traps, control-flow lowering, and runtime boundaries.
The experimental QBE emitter lowers target traps to distinct process exits
followed by `hlt`: `101` for checked `i32` overflow, `102` for division by
zero, and `103` for signed division overflow (`i32::MIN / -1`). The `Backend`
trait is the boundary used by the textual target emitter and experimental QBE
emitter; Cranelift output is intentionally not implemented.

## Public release posture

Vex is suitable for a labeled `0.1.0-alpha.1` public release as an interpreter
and typed-IR prototype. It is not presented as a production compiler. The MIT
license grants redistribution rights, and security reports should be sent to
`security@cybercoretech.net`. Native executable artifacts, a fuzzing service, maps,
generics, modules, and project configuration remain post-alpha work.
