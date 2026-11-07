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
| 1 | `||` | Boolean or |
| 2 | `&&` | Boolean and |
| 3 | `==`, `<`, `>` | Equality and numeric comparison |
| 4 | `+`, `-` | Addition and subtraction; `+` also joins strings |
| 5 | `*`, `/` | Multiplication and division |
| 6 | unary `-`, `!` | Numeric negation and boolean not |

Parentheses override precedence. `&&` and `||` short-circuit. Arithmetic is
checked for overflow, and division by zero is a runtime error.

## Strings

`+` joins two strings. It does not convert other values, so turn numbers and
booleans into text with `string(value)` first:

```vex
let name = "Vex";
let count = 3;
println("hello, " + name + "!");             // hello, Vex!
println("count: " + string(count));          // count: 3
println("ready: " + string(count > 2));      // ready: true
```

`"n=" + 1` is a type error that points you to `string(...)`. `string()`
accepts `i32`, `u64`, `bool` and `string` values. Strings compare with `==`.

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
are available as `i32(value)` and `u64(value)`, and `string(value)` turns a
number or boolean into text.

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
also available through the limited `map` and `map_get` built-ins. `use`
imports modules and packages (see [Modules and packages](packages.md)).
Generics and project execution remain deferred.

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
source = "src/main.vex"   # program entry
lib = "src/lib.vex"       # what `use sample;` loads when installed as a package
description = "One line"

[dependencies]
greeter = "0.1.0"         # exact versions
```

Use `vexlang project vex.toml` to validate and summarize it. Dependencies are
installed with `vexlang install`; see [Modules and packages](packages.md).

## Maps (scoped collection)

Maps are available as a deliberately small, non-generic collection:
`map("key", value, ...)` creates a string-keyed map and `map_get(map, "key")`
retrieves a value. Missing keys are runtime errors. Mutation, arbitrary key
types, iteration, and full generic type parameters remain deferred.
