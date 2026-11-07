# Modules and packages

## Modules

`use` makes another module's declarations available:

```vex
use math;           // math's functions, callable as `square(..)` or `math::square(..)`
use math::*;        // the same
use math::square;   // only `square` unqualified; others as `math::cube(..)`
```

A module named `math` is found in this order:

1. an inline `module math { ... }` block in the same file,
2. a file `math.vex` next to the importing file,
3. an installed package, `vex_modules/math/`, using its `vex.toml` `lib` entry
   (default `src/lib.vex`). The project root is the nearest directory above
   the source file that contains a `vex.toml`.

The rules in this first version are deliberately simple:

- **Functions are namespaced.** Every public function of an imported module
  can be called as `module::function(...)`. `use module;` also makes them
  callable unqualified, and `use module::function;` does that for one
  function and takes precedence over glob imports. A function your program
  declares itself wins over an imported one with the same name, and a
  module's private helpers never clash with anything. If two glob-imported
  modules both provide `min`, calling plain `min(..)` is an ambiguity error
  (`E1104`), and `a::min(..)` / `b::min(..)` both work.
- **Types share one namespace.** Records, enums, traits, newtypes,
  contracts, `impl` methods and `extern`/`trusted` functions keep their
  names. If two modules, or a module and your program, declare the same one,
  that's an error that names both.
- **Declarations only.** A module may contain `fn`, `record`, `enum`, `trait`,
  `impl`, `newtype`, `contract`, `use`, `export`, `module`, `test` and `bench`.
  Top-level statements are rejected, so importing never runs code. `test` and
  `bench` items are not imported.
- **Exports.** If a module has any `export NAME;` statements, only exported
  functions are imported or callable as `module::item`, and `use mod::item`
  may only name exported items. A module without exports is fully public.
- In generated Rust and QBE, `math::square` is named `math__square`.
- Each module loads once. Import cycles are reported as errors.
- Imported code works in every command: `run`, `check`, `test`, `build`/`ir`,
  `target`/`qbe` (within the scalar subset) and `rust`.

| Code | Meaning |
| --- | --- |
| `E1101` | module or item not found, `module::f` for a module this file doesn't import, or an unsupported nested path |
| `E1102` | the module failed to parse or check, or has top-level statements |
| `E1103` | import cycle |
| `E1104` | a type is declared in two places, or an unqualified call is ambiguous |
| `E1105` | item not exported, or an export of an undeclared name |

## Packages

Packages are listed in a static Git index,
[cybercoretech/vex-index](https://github.com/cybercoretech/vex-index). It's a
single `index.json` that records the tarball URL, SHA-256 and size of every
version. There is no registry server and no login.

### Using packages

```sh
vexlang new app && cd app
vexlang add greeter            # latest non-yanked version, or greeter@0.1.0
vexlang install                # after cloning: install what vex.toml lists
vexlang install --locked       # CI: fail if vex.lock would change
vexlang install --offline      # use only vex.lock and the download cache
vexlang remove greeter
vexlang search greet
vexlang info greeter
```

`vex.toml` lists exact versions:

```toml
[project]
name = "app"
version = "0.1.0"

[dependencies]
greeter = "0.1.0"
```

`install` resolves dependencies transitively, using the dependencies each
package declares in its own `vex.toml`. Two different versions of one package
anywhere in the tree are a conflict. Every tarball is checked against its
size and SHA-256 before anything is unpacked into `vex_modules/NAME/`.
Archives with paths outside the package, links or special files are refused.
`vex.lock` pins the URL and hash of every package. Commit `vex.lock`, and add
`vex_modules/` to `.gitignore` (`vexlang new` does both for you).

### Publishing a library

```sh
vexlang new greeter --lib      # src/lib.vex with an exported function
vexlang package                # dist/greeter-0.1.0.tar.gz (reproducible)
gh release create v0.1.0 dist/greeter-0.1.0.tar.gz
vexlang publish --index ../vex-index \
  --url https://github.com/OWNER/greeter/releases/download/v0.1.0/greeter-0.1.0.tar.gz
# commit index.json in vex-index and open a pull request
```

Published versions are immutable. To withdraw one, run
`vexlang yank NAME@VERSION --index ../vex-index` and open a pull request.
Yanked versions stay installable from an existing `vex.lock`, but `add` won't
pick them.
