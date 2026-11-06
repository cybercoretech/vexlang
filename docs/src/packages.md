# Modules and packages

## Modules

`use` makes another module's declarations available:

```vex
use math;           // everything in module `math`
use math::*;        // the same
use math::square;   // checks that `square` exists (and is exported)
```

A module named `math` is found in this order:

1. an inline `module math { ... }` block in the same file,
2. a file `math.vex` next to the importing file,
3. an installed package, `vex_modules/math/`, using its `vex.toml` `lib` entry
   (default `src/lib.vex`). The project root is the nearest directory above
   the source file that contains a `vex.toml`.

The rules in this first version are deliberately simple:

- **One namespace.** Imported declarations keep their names. If two modules,
  or a module and your program, declare the same name, that's an error that
  names both.
- **Declarations only.** A module may contain `fn`, `record`, `enum`, `trait`,
  `impl`, `newtype`, `contract`, `use`, `export`, `module`, `test` and `bench`.
  Top-level statements are rejected, so importing never runs code. `test` and
  `bench` items are not imported.
- **Exports.** If a module has any `export NAME;` statements, `use mod::item`
  may only name exported items. A module without exports is fully public.
- Each module loads once. Import cycles are reported as errors.
- Imported code works in every command: `run`, `check`, `test`, `build`/`ir`,
  `target`/`qbe` (within the scalar subset) and `rust`.

| Code | Meaning |
| --- | --- |
| `E1101` | module or item not found, or an unsupported nested path |
| `E1102` | the module failed to parse or check, or has top-level statements |
| `E1103` | import cycle |
| `E1104` | the same name is declared in two places |
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
