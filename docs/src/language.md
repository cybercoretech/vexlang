# Language reference

Vex source is a sequence of statements. Statements end with `;`, except
function declarations and block-bodied control-flow statements. Whitespace and
`//` comments are ignored.

## Literals and names

```vex
42;                 // inferred signed integer
true;               // boolean
"hello";            // string
let count = 3;
let total: i32 = count + 2;
let bytes: u64 = u64(9);
```

Identifiers begin with a letter or `_` and may contain letters, digits, and
underscores. The keywords `let`, `if`, `else`, `while`, `break`, `continue`,
`fn`, `return`, and `record` cannot be used as identifiers.

## Expressions and operators

Operators, from lowest to highest precedence, are:

| Precedence | Operators | Meaning |
| --- | --- | --- |
| 1 | `&&`, `||`, `==` | Boolean logic and equality |
| 2 | `<`, `>` | Numeric comparison |
| 3 | `+`, `-` | Addition and subtraction |
| 4 | `*`, `/` | Multiplication and division |
| 5 | unary `-`, `!` | Numeric negation and boolean not |

Parentheses override precedence. `&&` and `||` short-circuit. Arithmetic is
checked for overflow, and division by zero is a runtime error.

## Declarations and assignment

`let name = expression;` infers a type. An optional annotation can be
`i32`, `u64`, `bool`, `string`, or `res` (the latter is currently rejected as
unsupported). Assignment updates an existing variable:

```vex
let index: i32 = 0;
index = index + 1;
```

`i32` values must fit the signed 32-bit range. `u64` values are represented
separately at runtime and cannot contain negative values. Explicit conversions
are available as `i32(value)` and `u64(value)`.

## Records

Records declare fixed named fields. Construction must provide every field
exactly once with a compatible value, and unknown field access is rejected:

```vex
record Point { x: i32, y: i32 }
let point: Point = Point { x: 2, y: 3 };
point.x + point.y;
```

Records are supported by the interpreter and textual IR. Sum types use `enum`
declarations and construct tagged values with `EnumName::Variant`:

```vex
enum Maybe { None, Some { value: i32 } }
let answer: Maybe = Maybe::Some { value: 42 };
```

Enum variants are checked against their declared fields and preserved as tagged
runtime values and textual IR. The interpreter supports enum matching with
variant and wildcard patterns. The built-ins `ok(value)` and `err(value)`
create structured results; `match` checks `Ok`/`Err` exhaustiveness and `?`
propagates an error result from a function.
Arrays and indexing are supported as described below. String-keyed maps are
also available through the limited `map` and `map_get` built-ins. Modules,
imports, generics, and project execution remain deferred.

## Arrays and indexing

Array literals use square brackets and may be empty, nested, or assigned an
element type:

```vex
let numbers: [i32] = [1, 2, 3];
let matrix = [[1, 2], [3, 4]];
numbers[1] = 9;
numbers[1];
```

Indices are zero-based `i32` values. Indexing a non-array, using a non-integer
index, or selecting an out-of-bounds element is an error. Arrays are mutable;
map mutation, iteration, and generic collection types are deferred.

## Control flow

```vex
if ready { println("go"); } else { println("wait"); }

while index < 3 {
    index = index + 1;
}
```

Blocks may contain declarations and statements, followed by an optional final
expression. `break` and `continue` are valid only inside a loop. Loops stop
after one million iterations.

## Functions

Functions have typed parameters and an optional return type:

```vex
fn add(left: i32, right: i32) -> i32 {
    left + right
}

add(2, 3);
```

Functions can recurse, return explicitly with `return expression;`, and call
other declared functions. Active call depth is limited to 256. Built-ins
`print(value)` and `println(value)` write to standard output and return unit.

## Program result

The interpreter evaluates top-level statements in order and prints the final
value for `run`. `check` performs parsing and semantic analysis without
evaluation.

## Projects

Project roots may contain a small `vex.toml` foundation:

```toml
[project]
name = "sample"
version = "0.1.0"
source = "src/main.vex"
```

Use `vexlang project vex.toml` to validate and summarize it. Module/import
resolution and dependency management are reserved for a later phase.

## Maps (scoped collection)

Maps are available as a deliberately small, non-generic collection:
`map("key", value, ...)` creates a string-keyed map and `map_get(map, "key")`
retrieves a value. Missing keys are runtime errors. Mutation, arbitrary key
types, iteration, and full generic type parameters remain deferred.
