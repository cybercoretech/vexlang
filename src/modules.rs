//! Module resolution for `use`.
//!
//! `use NAME;`, `use NAME::*;` and `use NAME::item;` load module `NAME` and
//! make its declarations available to the importing program. `NAME` is looked
//! up in this order:
//!
//! 1. an inline `module NAME { ... }` block in the importing file,
//! 2. `NAME.vex` next to the importing file,
//! 3. an installed package: `<project>/vex_modules/NAME/`, entry file from
//!    its `vex.toml` `lib` key (default `src/lib.vex`).
//!
//! Version 1 semantics, kept deliberately simple:
//!
//! - **Flat namespace.** A module's declarations are merged into the program
//!   under their own names. Two modules (or a module and the program) that
//!   declare the same name are an error that names both.
//! - **Declarations only.** A module may contain `fn`, `record`, `enum`,
//!   `trait`, `impl`, `newtype`, `contract`, `use`, `export`, `module`,
//!   `test` and `bench`. Top-level statements are rejected so that importing
//!   never runs code. `test`/`bench` items in a module are not imported.
//! - **Exports.** If a module has `export` statements, `use NAME::item` may
//!   only name exported items. Without exports, every item is public.
//! - Each module is loaded once; import cycles are an error.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::analyzer::SemanticAnalyzer;
use crate::ast::Stmt;
use crate::builder::build_ast_with_spans;
use crate::diagnostics::{Diagnostic, span_for};
use crate::parser::parse_vex;

/// Directory, relative to the project root, that holds installed packages.
pub const MODULES_DIR: &str = "vex_modules";

#[derive(Debug, Clone)]
pub struct ModuleContext {
    /// Directory of the importing file (or the working directory for stdin).
    pub base_dir: PathBuf,
    /// `<project>/vex_modules`, if a project root was found.
    pub modules_dir: Option<PathBuf>,
}

thread_local! {
    static CONTEXT: RefCell<Option<ModuleContext>> = const { RefCell::new(None) };
}

impl ModuleContext {
    /// Context for a source file (`None` = stdin): its directory, plus the
    /// `vex_modules` of the nearest ancestor that has a `vex.toml`.
    pub fn for_path(path: Option<&str>) -> Self {
        let base_dir = path
            .filter(|p| *p != "-")
            .and_then(|p| Path::new(p).parent().map(Path::to_path_buf))
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."));
        let absolute = fs::canonicalize(&base_dir).unwrap_or_else(|_| base_dir.clone());
        let modules_dir = absolute
            .ancestors()
            .find(|dir| dir.join("vex.toml").is_file())
            .map(|root| root.join(MODULES_DIR));
        ModuleContext {
            base_dir,
            modules_dir,
        }
    }
}

/// Sets the context used by [`resolve`] on this thread.
pub fn set_context(context: ModuleContext) {
    CONTEXT.with(|c| *c.borrow_mut() = Some(context));
}

fn current_context() -> ModuleContext {
    CONTEXT
        .with(|c| c.borrow().clone())
        .unwrap_or_else(|| ModuleContext::for_path(None))
}

/// Declarations to prepend to `program` so that its `use` statements resolve.
/// Returns an empty list when the program has no `use`.
pub fn resolve(program: &[Stmt], source: &str) -> Result<Vec<Stmt>, Diagnostic> {
    if !program.iter().any(|s| matches!(s, Stmt::Use { .. })) {
        return Ok(Vec::new());
    }
    let context = current_context();
    let mut loader = Loader {
        modules_dir: context.modules_dir.clone(),
        loaded: BTreeMap::new(),
        owners: BTreeMap::new(),
        stack: Vec::new(),
        prelude: Vec::new(),
    };
    for stmt in program {
        if let Stmt::Use { path } = stmt {
            let anchor = span_for(source, Some(&format!("use {}", path.join("::"))));
            loader
                .import(path, &context.base_dir, program)
                .map_err(|error| error.into_diagnostic(anchor))?;
        }
    }
    // The program's own declarations must not clash with imported ones.
    for name in program.iter().flat_map(declared_names) {
        if let Some(owner) = loader.owners.get(&name) {
            return Err(Diagnostic::new(
                "E1104",
                format!("`{name}` is declared here and also imported from module `{owner}`"),
                span_for(source, Some(&name)),
            )
            .with_suggestion("rename one of them; modules share one namespace for now"));
        }
    }
    Ok(loader.prelude)
}

struct LoadError {
    code: &'static str,
    message: String,
    suggestion: Option<String>,
}

impl LoadError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        LoadError {
            code,
            message: message.into(),
            suggestion: None,
        }
    }

    fn hint(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    fn into_diagnostic(self, span: crate::diagnostics::Span) -> Diagnostic {
        let diagnostic = Diagnostic::new(self.code, self.message, span);
        match self.suggestion {
            Some(s) => diagnostic.with_suggestion(s),
            None => diagnostic,
        }
    }
}

struct LoadedModule {
    items: BTreeSet<String>,
    exports: BTreeSet<String>,
}

struct Loader {
    modules_dir: Option<PathBuf>,
    loaded: BTreeMap<String, LoadedModule>,
    /// Declared name -> module that declared it.
    owners: BTreeMap<String, String>,
    stack: Vec<String>,
    prelude: Vec<Stmt>,
}

/// Where a module's code comes from.
enum Origin {
    Inline(Vec<Stmt>),
    File(PathBuf),
}

impl Loader {
    fn import(
        &mut self,
        path: &[String],
        from_dir: &Path,
        siblings: &[Stmt],
    ) -> Result<(), LoadError> {
        let (name, item) = match path {
            [name] => (name, None),
            [name, item] => (name, Some(item)),
            _ => {
                return Err(LoadError::new(
                    "E1101",
                    format!(
                        "nested module paths are not supported yet: `use {}`",
                        path.join("::")
                    ),
                )
                .hint("import the module directly, e.g. `use NAME::item;`"));
            }
        };
        self.load(name, from_dir, siblings)?;
        if let Some(item) = item {
            let module = &self.loaded[name];
            if !module.items.contains(item) {
                return Err(LoadError::new(
                    "E1101",
                    format!("module `{name}` has no item `{item}`"),
                ));
            }
            if !module.exports.is_empty() && !module.exports.contains(item) {
                return Err(LoadError::new(
                    "E1105",
                    format!("`{item}` is not exported by module `{name}`"),
                )
                .hint(format!("add `export {item};` to module `{name}`")));
            }
        }
        Ok(())
    }

    fn find(&self, name: &str, from_dir: &Path, siblings: &[Stmt]) -> Result<Origin, LoadError> {
        for stmt in siblings {
            if let Stmt::Module { name: inline, body } = stmt {
                if inline == name {
                    return Ok(Origin::Inline(body.clone()));
                }
            }
        }
        let local = from_dir.join(format!("{name}.vex"));
        if local.is_file() {
            return Ok(Origin::File(local));
        }
        if let Some(modules) = &self.modules_dir {
            let package = modules.join(name);
            if package.is_dir() {
                let manifest = package.join("vex.toml");
                let lib = if manifest.is_file() {
                    crate::project::load(&manifest)
                        .map_err(|e| LoadError::new("E1102", format!("package `{name}`: {e}")))?
                        .lib
                } else {
                    crate::project::DEFAULT_LIB.to_string()
                };
                let entry = package.join(&lib);
                if !entry.is_file() {
                    return Err(LoadError::new(
                        "E1101",
                        format!("package `{name}` is installed but has no library file `{lib}`"),
                    ));
                }
                return Ok(Origin::File(entry));
            }
        }
        Err(
            LoadError::new("E1101", format!("cannot find module `{name}`")).hint(format!(
                "create `{name}.vex` next to this file, declare `module {name} {{ ... }}`, \
             or install a package with `vexlang add {name}`"
            )),
        )
    }

    fn load(&mut self, name: &str, from_dir: &Path, siblings: &[Stmt]) -> Result<(), LoadError> {
        if self.loaded.contains_key(name) {
            return Ok(());
        }
        if let Some(start) = self.stack.iter().position(|n| n == name) {
            let mut cycle = self.stack[start..].to_vec();
            cycle.push(name.to_string());
            return Err(LoadError::new(
                "E1103",
                format!("import cycle: {}", cycle.join(" -> ")),
            ));
        }

        let (body, dir, label) = match self.find(name, from_dir, siblings)? {
            Origin::Inline(body) => (body, from_dir.to_path_buf(), format!("module `{name}`")),
            Origin::File(path) => {
                let label = format!("module `{name}` ({})", path.display());
                let source = fs::read_to_string(&path)
                    .map_err(|e| LoadError::new("E1102", format!("{label}: {e}")))?;
                let body = parse_module(&source, &path)
                    .map_err(|e| LoadError::new("E1102", format!("error in {label}:\n{e}")))?;
                let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
                (body, dir, label)
            }
        };

        for stmt in &body {
            if !is_declaration(stmt) {
                return Err(LoadError::new(
                    "E1102",
                    format!("{label} may only contain declarations, but has a top-level statement"),
                )
                .hint("move executable code into a function"));
            }
        }

        self.stack.push(name.to_string());
        for stmt in &body {
            if let Stmt::Use { path } = stmt {
                self.import(path, &dir, &body)?;
            }
        }
        self.stack.pop();

        let own: Vec<Stmt> = body.iter().filter(|s| is_imported(s)).cloned().collect();
        let mut items = BTreeSet::new();
        for item in own.iter().flat_map(declared_names) {
            if let Some(owner) = self.owners.get(&item) {
                return Err(LoadError::new(
                    "E1104",
                    format!("`{item}` is declared by both module `{owner}` and module `{name}`"),
                )
                .hint("modules share one namespace for now; rename one of them"));
            }
            items.insert(item);
        }

        // Check the module on its own (with its imports) before merging it.
        let mut check = self.prelude.clone();
        check.extend(own.iter().cloned());
        SemanticAnalyzer::new()
            .analyze(&check)
            .map_err(|e| LoadError::new("E1102", format!("error in {label}: {e}")))?;

        let exports = body
            .iter()
            .filter_map(|s| match s {
                Stmt::Export { name } => Some(name.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        if let Some(missing) = exports.iter().find(|e| !items.contains(*e)) {
            return Err(LoadError::new(
                "E1105",
                format!("{label} exports `{missing}`, which it does not declare"),
            ));
        }

        for item in &items {
            self.owners.insert(item.clone(), name.to_string());
        }
        self.prelude.extend(own);
        self.loaded
            .insert(name.to_string(), LoadedModule { items, exports });
        Ok(())
    }
}

fn parse_module(source: &str, path: &Path) -> Result<Vec<Stmt>, String> {
    let display = path.display().to_string();
    let pairs = parse_vex(source).map_err(|e| e.to_string())?;
    let stmts = build_ast_with_spans(pairs).map_err(|e| {
        let message = e.to_string();
        let needle = message.split('`').nth(1);
        Diagnostic::new("E1002", message.clone(), span_for(source, needle)).render(source, &display)
    })?;
    Ok(stmts.into_iter().map(|s| s.stmt).collect())
}

fn is_declaration(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::Function { .. }
            | Stmt::Record { .. }
            | Stmt::Enum { .. }
            | Stmt::Trait { .. }
            | Stmt::Impl { .. }
            | Stmt::Newtype { .. }
            | Stmt::Contract { .. }
            | Stmt::Use { .. }
            | Stmt::Export { .. }
            | Stmt::Module { .. }
            | Stmt::Test(_)
            | Stmt::Bench(_)
            | Stmt::Extern { .. }
            | Stmt::Trusted(_)
    )
}

/// Declarations that are merged into the importer.
fn is_imported(stmt: &Stmt) -> bool {
    is_declaration(stmt)
        && !matches!(
            stmt,
            Stmt::Use { .. }
                | Stmt::Export { .. }
                | Stmt::Module { .. }
                | Stmt::Test(_)
                | Stmt::Bench(_)
        )
}

fn declared_names(stmt: &Stmt) -> Vec<String> {
    match stmt {
        Stmt::Function { name, .. }
        | Stmt::Record { name, .. }
        | Stmt::Enum { name, .. }
        | Stmt::Trait { name, .. }
        | Stmt::Newtype { name, .. }
        | Stmt::Contract { name, .. } => vec![name.clone()],
        Stmt::Impl { methods, .. } => methods.iter().flat_map(declared_names).collect(),
        Stmt::Extern { func, .. } | Stmt::Trusted(func) => declared_names(func),
        _ => Vec::new(),
    }
}
