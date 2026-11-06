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
//! Semantics:
//!
//! - **Functions are namespaced.** A module's functions can always be called
//!   as `NAME::function(...)`. `use NAME;` (or `use NAME::*;`) also makes
//!   them callable unqualified, and `use NAME::function;` makes just that
//!   one callable unqualified (it takes precedence over glob imports). Internally they are renamed `NAME::function`,
//!   so equal names in different modules never collide: a function the
//!   program declares itself wins over a glob import, and a name that two
//!   globs provide is an error only when it is called unqualified.
//! - **Types share one namespace.** Records, enums, traits, newtypes,
//!   contracts, `impl` methods and `extern`/`trusted` functions keep their
//!   names. Two modules (or a module and the program) that declare the same
//!   such name are an error that names both.
//! - **Declarations only.** A module may contain `fn`, `record`, `enum`,
//!   `trait`, `impl`, `newtype`, `contract`, `use`, `export`, `module`,
//!   `test` and `bench`. Top-level statements are rejected so that importing
//!   never runs code. `test`/`bench` items in a module are not imported.
//! - **Exports.** If a module has `export` statements, only exported items
//!   can be imported or called as `NAME::item`. Without exports, every item
//!   is public.
//! - Each module is loaded once; import cycles are an error.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::analyzer::SemanticAnalyzer;
use crate::ast::{Expr, Stmt};
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

/// Resolves the program's `use` statements and qualified calls.
///
/// Returns the imported declarations followed by the program itself, with
/// `use` statements removed (backends never see a `use` they cannot
/// express) and every call to a module function renamed to the
/// `module::function` name it is declared under.
pub fn resolve(program: Vec<Stmt>, source: &str) -> Result<Vec<Stmt>, Diagnostic> {
    let uses_modules = program.iter().any(|s| matches!(s, Stmt::Use { .. }));
    let mut qualified = false;
    for stmt in &program {
        visit_calls(stmt, &mut |name| {
            qualified |= name.contains("::");
        });
    }
    if !uses_modules && !qualified {
        return Ok(program);
    }
    let context = current_context();
    let mut loader = Loader {
        modules_dir: context.modules_dir.clone(),
        loaded: BTreeMap::new(),
        owners: BTreeMap::new(),
        stack: Vec::new(),
        prelude: Vec::new(),
    };
    let mut scope = Scope::new(None, &program);
    for stmt in &program {
        if let Stmt::Use { path } = stmt {
            let anchor = span_for(source, Some(&format!("use {}", path.join("::"))));
            loader
                .import(path, &context.base_dir, &program, &mut scope)
                .map_err(|error| error.into_diagnostic(anchor))?;
        }
    }
    // The program's own declarations must not clash with imported types.
    for name in program.iter().flat_map(shared_names) {
        if let Some(owner) = loader.owners.get(&name) {
            return Err(Diagnostic::new(
                "E1104",
                format!("`{name}` is declared here and also imported from module `{owner}`"),
                span_for(source, Some(&name)),
            )
            .with_suggestion("rename one of them; types share one namespace for now"));
        }
    }
    let mut stmts = loader.prelude;
    for mut stmt in program {
        if matches!(stmt, Stmt::Use { .. }) {
            continue;
        }
        let mut failure = None;
        rename_calls(&mut stmt, &mut |name| {
            if failure.is_none() {
                match scope.lookup(name, &loader.loaded) {
                    Ok(Some(target)) => *name = target,
                    Ok(None) => {}
                    Err(error) => failure = Some((name.clone(), error)),
                }
            }
        });
        if let Some((name, error)) = failure {
            return Err(error.into_diagnostic(span_for(source, Some(&format!("{name}(")))));
        }
        stmts.push(stmt);
    }
    Ok(stmts)
}

/// The names one body (the program or a module) can call.
struct Scope {
    /// `Some(module)` inside a module: its functions are renamed `module::f`.
    module: Option<String>,
    /// Plain functions this body declares itself.
    own: BTreeSet<String>,
    /// Unqualified name -> `module::name`, from `use module::name;`.
    explicit: BTreeMap<String, String>,
    /// Unqualified name -> `module::name` candidates from glob imports.
    imported: BTreeMap<String, BTreeSet<String>>,
    /// Modules this body imports, usable as `module::f`.
    modules: BTreeSet<String>,
}

impl Scope {
    fn new(module: Option<String>, body: &[Stmt]) -> Self {
        Scope {
            module,
            own: body.iter().filter_map(function_name).collect(),
            explicit: BTreeMap::new(),
            imported: BTreeMap::new(),
            modules: BTreeSet::new(),
        }
    }

    /// The declared name a call to `name` refers to; `None` keeps it as is
    /// (builtins, `impl` methods, externs, or unknown names the analyzer
    /// reports).
    fn lookup(
        &self,
        name: &str,
        loaded: &BTreeMap<String, LoadedModule>,
    ) -> Result<Option<String>, LoadError> {
        if let Some((module, item)) = name.split_once("::") {
            if item.contains("::") {
                return Err(LoadError::new(
                    "E1101",
                    format!("nested module paths are not supported yet: `{name}`"),
                ));
            }
            if self.module.as_deref() == Some(module) && self.own.contains(item) {
                return Ok(Some(name.to_string()));
            }
            let Some(loaded) = loaded.get(module).filter(|_| self.modules.contains(module)) else {
                return Err(LoadError::new(
                    "E1101",
                    format!("module `{module}` is not imported here, so `{name}` is unknown"),
                )
                .hint(format!("add `use {module};` (or `use {module}::{item};`)")));
            };
            if !loaded.functions.contains(item) {
                let what = if loaded.items.contains(item) {
                    format!("`{item}` in module `{module}` is not a function")
                } else {
                    format!("module `{module}` has no function `{item}`")
                };
                return Err(LoadError::new("E1101", what));
            }
            if !loaded.is_public(item) {
                return Err(LoadError::new(
                    "E1105",
                    format!("`{item}` is not exported by module `{module}`"),
                )
                .hint(format!("add `export {item};` to module `{module}`")));
            }
            return Ok(Some(name.to_string()));
        }
        if self.own.contains(name) {
            return Ok(self.module.as_ref().map(|m| format!("{m}::{name}")));
        }
        if let Some(target) = self.explicit.get(name) {
            return Ok(Some(target.clone()));
        }
        match self.imported.get(name) {
            None => Ok(None),
            Some(targets) if targets.len() == 1 => Ok(targets.iter().next().cloned()),
            Some(targets) => {
                let modules: Vec<&str> = targets
                    .iter()
                    .filter_map(|t| t.split_once("::").map(|(m, _)| m))
                    .collect();
                Err(LoadError::new(
                    "E1104",
                    format!(
                        "`{name}` is ambiguous: it is imported from module `{}`",
                        modules.join("` and module `")
                    ),
                )
                .hint(format!(
                    "call it as `{}::{name}(...)`, or import only the one you need",
                    modules[0]
                )))
            }
        }
    }
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
    /// Every declared name (functions and shared names).
    items: BTreeSet<String>,
    /// Plain functions, declared as `module::name`.
    functions: BTreeSet<String>,
    exports: BTreeSet<String>,
}

impl LoadedModule {
    fn is_public(&self, item: &str) -> bool {
        self.exports.is_empty() || self.exports.contains(item)
    }
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
        scope: &mut Scope,
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
        let item = item.filter(|item| item.as_str() != "*");
        self.load(name, from_dir, siblings)?;
        let module = &self.loaded[name];
        scope.modules.insert(name.clone());
        match item {
            Some(item) => {
                if !module.items.contains(item) {
                    return Err(LoadError::new(
                        "E1101",
                        format!("module `{name}` has no item `{item}`"),
                    ));
                }
                if !module.is_public(item) {
                    return Err(LoadError::new(
                        "E1105",
                        format!("`{item}` is not exported by module `{name}`"),
                    )
                    .hint(format!("add `export {item};` to module `{name}`")));
                }
                if module.functions.contains(item) {
                    let target = format!("{name}::{item}");
                    if let Some(previous) = scope.explicit.get(item).filter(|p| **p != target) {
                        return Err(LoadError::new(
                            "E1104",
                            format!("`{item}` is imported as both `{previous}` and `{target}`"),
                        )
                        .hint("import one of them and call the other with its module path"));
                    }
                    scope.explicit.insert(item.clone(), target);
                }
            }
            None => {
                for function in module.functions.iter().filter(|f| module.is_public(f)) {
                    scope
                        .imported
                        .entry(function.clone())
                        .or_default()
                        .insert(format!("{name}::{function}"));
                }
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

        let mut scope = Scope::new(Some(name.to_string()), &body);
        self.stack.push(name.to_string());
        for stmt in &body {
            if let Stmt::Use { path } = stmt {
                self.import(path, &dir, &body, &mut scope)?;
            }
        }
        self.stack.pop();

        let mut own: Vec<Stmt> = body.iter().filter(|s| is_imported(s)).cloned().collect();
        let mut shared = BTreeSet::new();
        for item in own.iter().flat_map(shared_names) {
            if let Some(owner) = self.owners.get(&item) {
                return Err(LoadError::new(
                    "E1104",
                    format!("`{item}` is declared by both module `{owner}` and module `{name}`"),
                )
                .hint("types share one namespace for now; rename one of them"));
            }
            shared.insert(item);
        }
        let functions = scope.own.clone();
        for stmt in &mut own {
            if let Stmt::Function { name: declared, .. } = stmt {
                *declared = format!("{name}::{declared}");
            }
            let mut failure = None;
            rename_calls(stmt, &mut |call| {
                if failure.is_none() {
                    match scope.lookup(call, &self.loaded) {
                        Ok(Some(target)) => *call = target,
                        Ok(None) => {}
                        Err(error) => failure = Some(error),
                    }
                }
            });
            if let Some(error) = failure {
                return Err(LoadError {
                    message: format!("in {label}: {}", error.message),
                    ..error
                });
            }
        }
        let mut items = shared.clone();
        items.extend(functions.iter().cloned());

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

        for item in shared {
            self.owners.insert(item, name.to_string());
        }
        self.prelude.extend(own);
        self.loaded.insert(
            name.to_string(),
            LoadedModule {
                items,
                functions,
                exports,
            },
        );
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

/// A plain (namespaced) function declared by `stmt`.
fn function_name(stmt: &Stmt) -> Option<String> {
    match stmt {
        Stmt::Function { name, .. } => Some(name.clone()),
        _ => None,
    }
}

/// Names declared by `stmt` that live in the one shared namespace.
fn shared_names(stmt: &Stmt) -> Vec<String> {
    match stmt {
        Stmt::Record { name, .. }
        | Stmt::Enum { name, .. }
        | Stmt::Trait { name, .. }
        | Stmt::Newtype { name, .. }
        | Stmt::Contract { name, .. } => vec![name.clone()],
        Stmt::Impl { methods, .. } => methods.iter().flat_map(function_name).collect(),
        Stmt::Extern { func, .. } | Stmt::Trusted(func) => {
            function_name(func).into_iter().collect()
        }
        _ => Vec::new(),
    }
}

/// Calls `f` with the name of every call in `stmt`.
fn visit_calls(stmt: &Stmt, f: &mut dyn FnMut(&str)) {
    let mut copy = stmt.clone();
    rename_calls(&mut copy, &mut |name| f(name));
}

/// Calls `f` on the name of every call in `stmt` (not inside inline
/// `module` blocks, which are loaded separately).
fn rename_calls(stmt: &mut Stmt, f: &mut dyn FnMut(&mut String)) {
    match stmt {
        Stmt::Function { body, .. } => rename_expr(body, f),
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::ExprStmt(value) => {
            rename_expr(value, f)
        }
        Stmt::AssignIndex { indices, value, .. } => {
            indices.iter_mut().for_each(|e| rename_expr(e, f));
            rename_expr(value, f);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rename_expr(condition, f);
            rename_expr(then_branch, f);
            if let Some(e) = else_branch {
                rename_expr(e, f);
            }
        }
        Stmt::While { condition, body } => {
            rename_expr(condition, f);
            rename_expr(body, f);
        }
        Stmt::Return(Some(e)) | Stmt::Invariant(e) | Stmt::Unsafe(e) | Stmt::Throw(e) => {
            rename_expr(e, f)
        }
        Stmt::Impl { methods, .. } => methods.iter_mut().for_each(|m| rename_calls(m, f)),
        Stmt::Contract { invariants, .. } => invariants.iter_mut().for_each(|e| rename_expr(e, f)),
        Stmt::Test(inner) | Stmt::Bench(inner) | Stmt::Trusted(inner) => rename_calls(inner, f),
        Stmt::Extern { func, .. } => rename_calls(func, f),
        Stmt::Ensure { condition, message } => {
            rename_expr(condition, f);
            rename_expr(message, f);
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::Record { .. }
        | Stmt::Enum { .. }
        | Stmt::Trait { .. }
        | Stmt::Module { .. }
        | Stmt::Use { .. }
        | Stmt::Export { .. }
        | Stmt::Newtype { .. } => {}
    }
}

fn rename_expr(expr: &mut Expr, f: &mut dyn FnMut(&mut String)) {
    match expr {
        Expr::Call(name, args) => {
            f(name);
            args.iter_mut().for_each(|a| rename_expr(a, f));
        }
        Expr::BinaryOp(a, _, b) | Expr::Index(a, b) => {
            rename_expr(a, f);
            rename_expr(b, f);
        }
        Expr::UnaryOp(_, e) | Expr::Field(e, _) | Expr::Try(e) => rename_expr(e, f),
        Expr::Block(stmts, tail) => {
            stmts.iter_mut().for_each(|s| rename_calls(s, f));
            if let Some(t) = tail {
                rename_expr(t, f);
            }
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rename_expr(condition, f);
            rename_expr(then_branch, f);
            if let Some(e) = else_branch {
                rename_expr(e, f);
            }
        }
        Expr::Record(_, fields) | Expr::EnumVariant { fields, .. } => {
            fields.iter_mut().for_each(|(_, e)| rename_expr(e, f))
        }
        Expr::Array(items) => items.iter_mut().for_each(|e| rename_expr(e, f)),
        Expr::Match { value, arms } => {
            rename_expr(value, f);
            arms.iter_mut().for_each(|(_, e)| rename_expr(e, f));
        }
        Expr::Int(_) | Expr::String(_) | Expr::Bool(_) | Expr::Var(_) => {}
    }
}
