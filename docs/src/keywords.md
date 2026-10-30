# Vexlang Custom Language Keywords & Syntax Elements

Vexlang introduces a clean, expressive set of keywords and syntax elements designed for systems programming, safe code lowering, and modern ergonomics without complex borrow checker overhead.

## Core Declarations & Structure

### `let` - Variable Binding
Declare immutable or mutable local bindings with optional type annotations.
```vex
let base: i32 = 10;
let mut count = 0;
```

### `fn` - Function Definition
Define typed functions with parameters, return types, and expression bodies.
```vex
fn add(a: i32, b: i32) -> i32 {
    a + b
}
```

### `record` - Structured Data
Define user-defined record types with typed fields.
```vex
record Point {
    x: i32,
    y: i32,
}
```

### `enum` - Algebraic Data Types
Define enums with unit variants or payload fields.
```vex
enum Status {
    Active,
    Inactive,
    Error { code: i32 },
}
```

### `trait` - Interfaces & Behaviors
Define shared behavior contracts and method signatures.
```vex
trait Summary {
    fn summarize() -> string;
}
```

### `impl` - Implementation Blocks
Implement methods, trait implementations, or inherent functions on types.
```vex
impl Point {
    fn distance() -> i32 {
        self.x * self.x + self.y * self.y
    }
}
```

### `mod` - Modules & Namespaces
Organize code into hierarchical submodules and namespaces.
```vex
mod math {
    fn square(x: i32) -> i32 { x * x }
}
```

### `use` & `export` - Imports & Visibility
Import paths and export symbols across module boundaries.
```vex
use math::square;
export square;
```

### `contract` & `invariant` - Formal Verification
Define program contracts, preconditions, postconditions, and invariants.
```vex
contract SecureAccount {
    invariant balance >= 0;
}
```

### `newtype` - Zero-Cost Wrapper Types
Define distinct wrapper types around underlying primitives.
```vex
newtype UserId(i32);
```

---

## Control Flow & Error Handling

- **`if` / `else`**: Conditional branching expressions.
- **`while`**: Condition-checked loops with bounded iteration safety limits.
- **`break` / `continue`**: Loop control statements.
- **`match`**: Exhaustive pattern matching over enums, results, and values.
- **`?`**: Result error propagation operator (`let val = operation()?;`).
- **`ok` / `err`**: Structured result constructors (`ok(42)`, `err("failed")`).
