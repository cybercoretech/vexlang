use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, Read, Write};

use pest::error::LineColLocation;

mod analyzer;
mod ast;
mod builder;
#[allow(dead_code)]
mod codegen;
mod diagnostics;
mod evaluator;
mod ir;
mod parser;
mod project;
mod rust_codegen;

use analyzer::SemanticAnalyzer;
use builder::{SpannedStmt, build_ast_with_spans};
use codegen::{Backend, QbeBackend, TextBackend};
use diagnostics::{Diagnostic, Span, span_for};
use parser::parse_vex;

fn declaration_spans(source: &str, stmts: &[SpannedStmt]) -> HashMap<String, Span> {
    let mut spans = HashMap::new();
    for spanned in stmts {
        let names: Vec<&str> = match &spanned.stmt {
            ast::Stmt::Let { name, .. }
            | ast::Stmt::Function { name, .. }
            | ast::Stmt::Record { name, .. }
            | ast::Stmt::Enum { name, .. } => vec![name.as_str()],
            _ => Vec::new(),
        };
        let text = &source[spanned.span.start..spanned.span.end];
        for name in names {
            if let Some(offset) = text.find(name) {
                spans.entry(name.to_string()).or_insert(Span {
                    start: spanned.span.start + offset,
                    end: spanned.span.start + offset + name.len(),
                });
            }
        }
    }
    spans
}

fn parse_error_diagnostic(source: &str, error: pest::error::Error<parser::Rule>) -> Diagnostic {
    let (line, column) = match error.line_col {
        LineColLocation::Pos(position) => position,
        LineColLocation::Span(start, _) => start,
    };
    let offset = source
        .lines()
        .take(line.saturating_sub(1))
        .map(|l| l.len() + 1)
        .sum::<usize>()
        + column.saturating_sub(1);
    Diagnostic::new(
        "E1001",
        format!("parse error: {error}"),
        Span {
            start: offset,
            end: offset + 1,
        },
    )
    .with_suggestion("check the preceding expression and add a semicolon if needed")
}

fn semantic_diagnostic(
    error: String,
    source: &str,
    declarations: &HashMap<String, Span>,
) -> Diagnostic {
    let needle = error.split('`').nth(1);
    let mut diagnostic = Diagnostic::new("E2001", error.clone(), span_for(source, needle));
    if error.contains("undefined variable") {
        let span = diagnostic.span;
        diagnostic = diagnostic
            .with_label(span, "undefined name referenced here")
            .with_suggestion("declare the variable with `let` before using it");
    } else if error.contains("undefined function") {
        let span = diagnostic.span;
        diagnostic = diagnostic
            .with_label(span, "undefined function called here")
            .with_suggestion("declare the function with `fn` before calling it");
    } else if let Some(name) = needle {
        if let Some(span) = declarations.get(name) {
            diagnostic = diagnostic.with_label(*span, "declared here");
        }
    }

    if error.contains("boolean") {
        diagnostic = diagnostic.with_suggestion("use `true` or `false`, or compare numeric values");
    }

    diagnostic
}

fn recover_parse_diagnostics(source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut offset = 0;
    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            let candidate = if trimmed.ends_with(';') || trimmed.ends_with('}') {
                format!("{trimmed}\n")
            } else {
                format!("{trimmed};\n")
            };
            if let Err(error) = parse_vex(&candidate) {
                let column = line.find(trimmed).unwrap_or(0);
                diagnostics.push(
                    Diagnostic::new(
                        "E1001",
                        format!("parse error: {error}"),
                        Span {
                            start: offset + column,
                            end: offset + column + trimmed.len().max(1),
                        },
                    )
                    .with_label(
                        Span {
                            start: offset + column,
                            end: offset + column + trimmed.len().max(1),
                        },
                        "could not parse this line",
                    )
                    .with_suggestion("check this statement before continuing"),
                );
            }
        }
        offset += line.len() + 1;
    }
    diagnostics
}

fn pipeline(source: &str) -> Result<Vec<ast::Stmt>, Diagnostic> {
    let pairs = parse_vex(source).map_err(|error| parse_error_diagnostic(source, error))?;
    let spanned_stmts = build_ast_with_spans(pairs).map_err(|error| {
        let message = error.to_string();
        let needle = message.split('`').nth(1);
        Diagnostic::new("E1002", message.clone(), span_for(source, needle))
    })?;
    let declarations = declaration_spans(source, &spanned_stmts);
    let stmts = spanned_stmts
        .into_iter()
        .map(|spanned| spanned.stmt)
        .collect::<Vec<_>>();
    let mut analyzer = SemanticAnalyzer::new();
    analyzer
        .analyze(&stmts)
        .map_err(|error| semantic_diagnostic(error, source, &declarations))?;
    Ok(stmts)
}

fn pipeline_all(source: &str) -> Result<Vec<ast::Stmt>, Vec<Diagnostic>> {
    let pairs = match parse_vex(source) {
        Ok(pairs) => pairs,
        Err(error) => {
            let recovered = recover_parse_diagnostics(source);
            return Err(if recovered.len() > 1 {
                recovered
            } else {
                vec![parse_error_diagnostic(source, error)]
            });
        }
    };
    let spanned_stmts = build_ast_with_spans(pairs).map_err(|error| {
        let message = error.to_string();
        let needle = message.split('`').nth(1);
        vec![Diagnostic::new(
            "E1002",
            message.clone(),
            span_for(source, needle),
        )]
    })?;
    let declarations = declaration_spans(source, &spanned_stmts);
    let stmts = spanned_stmts
        .into_iter()
        .map(|spanned| spanned.stmt)
        .collect::<Vec<_>>();
    let mut analyzer = SemanticAnalyzer::new();
    let errors = analyzer.analyze_all(&stmts);
    if errors.is_empty() {
        Ok(stmts)
    } else {
        Err(errors
            .into_iter()
            .map(|error| semantic_diagnostic(error, source, &declarations))
            .collect())
    }
}

fn lower_source(source: &str) -> Result<(Vec<ast::Stmt>, ir::IrProgram), Diagnostic> {
    let stmts = pipeline(source)?;
    let program = ir::lower(&stmts).map_err(|error| {
        let message = error.to_string();
        let code = if message.starts_with("IR") {
            "E4003"
        } else {
            "E4002"
        };
        Diagnostic::new(
            code,
            format!("IR lowering error: {error}"),
            span_for(source, None),
        )
    })?;
    Ok((stmts, program))
}

fn run(source: &str) -> Result<ast::Value, Diagnostic> {
    let stmts = pipeline(source)?;
    evaluator::eval(&stmts).map_err(|error| {
        let message = error.to_string();
        let code = if matches!(error, evaluator::EvalError::TypeError(_)) {
            "E3002"
        } else {
            "E3001"
        };
        Diagnostic::new(code, message.clone(), span_for(source, None)).with_suggestion(
            if message.contains("division by zero") {
                "ensure the divisor is not zero"
            } else {
                "check the values used by this expression"
            },
        )
    })
}

fn apply_repl_input(
    source: &mut String,
    statements: &mut Vec<ast::Stmt>,
    session: &mut evaluator::Session,
    input: &str,
) -> Result<ast::Value, Diagnostic> {
    let mut candidate = source.clone();
    candidate.push_str(input);
    let parsed = pipeline(&candidate)?;
    let new_statements = parsed
        .get(statements.len()..)
        .ok_or_else(|| {
            Diagnostic::new(
                "E9001",
                "REPL statement history is inconsistent",
                Span { start: 0, end: 0 },
            )
        })?
        .to_vec();

    let previous_session = session.clone();
    match session.eval(&new_statements) {
        Ok(value) => {
            *source = candidate;
            *statements = parsed;
            Ok(value)
        }
        Err(error) => {
            *session = previous_session;
            let message = error.to_string();
            let code = if matches!(error, evaluator::EvalError::TypeError(_)) {
                "E3002"
            } else {
                "E3001"
            };
            Err(
                Diagnostic::new(code, message.clone(), span_for(&candidate, None)).with_suggestion(
                    if message.contains("division by zero") {
                        "ensure the divisor is not zero"
                    } else {
                        "check the values used by this expression"
                    },
                ),
            )
        }
    }
}

fn read_source(path: Option<&str>) -> Result<String, String> {
    match path {
        None | Some("-") => {
            let mut source = String::new();
            io::stdin()
                .read_to_string(&mut source)
                .map_err(|error| format!("could not read stdin: {error}"))?;
            Ok(source)
        }
        Some(path) => {
            fs::read_to_string(path).map_err(|error| format!("could not read `{path}`: {error}"))
        }
    }
}

fn usage() -> &'static str {
    "usage: vexlang [--json] [check|run|repl|project|fmt|test|build|ir|target|qbe|rust] [FILE|-]\n       vexlang FILE   (backwards-compatible alias for run)\n       vexlang --version"
}

fn render_diagnostic(error: &Diagnostic, source: &str, path: Option<&str>, json: bool) -> String {
    let name = path.unwrap_or("<stdin>");
    if json {
        error.render_json(source, name)
    } else {
        error.render(source, name)
    }
}

fn report_diagnostic(error: &Diagnostic, source: &str, path: Option<&str>, json: bool) {
    eprintln!("{}", render_diagnostic(error, source, path, json));
}

fn report_diagnostics(errors: &[Diagnostic], source: &str, path: Option<&str>, json: bool) {
    if json {
        let rendered = errors
            .iter()
            .map(|error| render_diagnostic(error, source, path, true))
            .collect::<Vec<_>>()
            .join(",");
        eprintln!("[{rendered}]");
    } else {
        for (index, error) in errors.iter().enumerate() {
            if index > 0 {
                eprintln!();
            }
            report_diagnostic(error, source, path, false);
        }
    }
}

fn repl() {
    println!("Vexlang REPL (enter `:help` for commands)");
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut line = String::new();
    let mut source = String::new();
    let mut statements = Vec::new();
    let mut pending = String::new();
    let mut session = evaluator::Session::new();
    loop {
        print!("vex> ");
        let _ = io::stdout().flush();
        line.clear();
        if input.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let command = line.trim();
        if pending.is_empty() && command.starts_with(':') {
            let mut parts = command.split_whitespace();
            match parts.next().unwrap_or_default() {
                ":quit" | ":q" => break,
                ":help" => println!(":help :reset :type EXPR :ir :load FILE :quit"),
                ":reset" => {
                    source.clear();
                    statements.clear();
                    session = evaluator::Session::new();
                    println!("reset");
                }
                ":type" => {
                    let expression = parts.collect::<Vec<_>>().join(" ");
                    let mut query = source.clone();
                    query.push_str(&format!("\n{expression};"));
                    match pipeline(&query) {
                        Ok(stmts) => println!("{:?}", stmts.last()),
                        Err(error) => eprintln!("{}", error.render(&query, "<repl>")),
                    }
                }
                ":ir" => match lower_source(&source) {
                    Ok((_, program)) => print!("{}", ir::render(&program)),
                    Err(error) => eprintln!("{}", error.render(&source, "<repl>")),
                },
                ":load" => {
                    if let Some(path) = parts.next() {
                        match fs::read_to_string(path) {
                            Ok(contents) => {
                                let mut input = contents;
                                if !input.ends_with('\n') {
                                    input.push('\n');
                                }
                                let diagnostic_source = format!("{source}{input}");
                                match apply_repl_input(
                                    &mut source,
                                    &mut statements,
                                    &mut session,
                                    &input,
                                ) {
                                    Ok(_) => println!("loaded {path}"),
                                    Err(error) => {
                                        eprintln!("{}", error.render(&diagnostic_source, "<repl>"))
                                    }
                                }
                            }
                            Err(error) => eprintln!("could not load `{path}`: {error}"),
                        }
                    } else {
                        eprintln!("usage: :load FILE");
                    }
                }
                _ => eprintln!("unknown command; enter :help"),
            }
            continue;
        }
        if command.is_empty() && pending.is_empty() {
            continue;
        }
        pending.push_str(&line);
        if pending.matches('{').count() != pending.matches('}').count() {
            continue;
        }
        let input = std::mem::take(&mut pending);
        let diagnostic_source = format!("{source}{input}");
        match apply_repl_input(&mut source, &mut statements, &mut session, &input) {
            Ok(value) => println!("{value:?}"),
            Err(error) => {
                eprintln!("{}", error.render(&diagnostic_source, "<repl>"));
            }
        }
    }
}

fn main() {
    let mut diagnostic_json = false;
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|arg| {
            if arg == "--json" || arg == "--diagnostic-format=json" {
                diagnostic_json = true;
                false
            } else {
                true
            }
        })
        .collect();
    if matches!(args.as_slice(), [flag] if flag == "--version" || flag == "-V") {
        println!("vexlang {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let (command, path) = match args.as_slice() {
        [] => ("run", None),
        [one]
            if !matches!(
                one.as_str(),
                "check"
                    | "run"
                    | "repl"
                    | "project"
                    | "fmt"
                    | "test"
                    | "build"
                    | "ir"
                    | "target"
                    | "qbe"
            ) =>
        {
            ("run", Some(one.as_str()))
        }
        [command] => (command.as_str(), None),
        [command, path]
            if matches!(
                command.as_str(),
                "check"
                    | "run"
                    | "project"
                    | "fmt"
                    | "test"
                    | "build"
                    | "ir"
                    | "target"
                    | "qbe"
                    | "rust"
            ) =>
        {
            (command.as_str(), Some(path.as_str()))
        }
        _ => {
            eprintln!("{}", usage());
            std::process::exit(2);
        }
    };
    if command == "repl" {
        repl();
        return;
    }
    if command == "project" {
        match project::load(std::path::Path::new(path.unwrap_or("vex.toml"))) {
            Ok(config) => println!("{} {} ({})", config.name, config.version, config.source),
            Err(error) => {
                eprintln!("error: {error}");
                std::process::exit(1);
            }
        }
        return;
    }
    let source = match read_source(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(2);
        }
    };
    match command {
        "check" => match pipeline_all(&source) {
            Ok(_) => println!("ok"),
            Err(errors) => {
                report_diagnostics(&errors, &source, path, diagnostic_json);
                std::process::exit(1);
            }
        },
        "run" | "test" => match pipeline(&source) {
            Ok(stmts) if command == "test" => match evaluator::eval(&stmts) {
                Ok(_) => println!("test passed"),
                Err(error) => {
                    let diagnostic =
                        Diagnostic::new("E3001", error.to_string(), span_for(&source, None));
                    report_diagnostic(&diagnostic, &source, path, diagnostic_json);
                    std::process::exit(1);
                }
            },
            Ok(_) => match run(&source) {
                Ok(value) => println!("{value:?}"),
                Err(error) => {
                    report_diagnostic(&error, &source, path, diagnostic_json);
                    std::process::exit(1);
                }
            },
            Err(error) => {
                report_diagnostic(&error, &source, path, diagnostic_json);
                std::process::exit(1);
            }
        },
        "build" | "ir" => match lower_source(&source) {
            Ok((_stmts, program)) => print!("{}", ir::render(&program)),
            Err(error) => {
                report_diagnostic(&error, &source, path, diagnostic_json);
                std::process::exit(1);
            }
        },
        "target" | "qbe" => match lower_source(&source) {
            Ok((_stmts, program)) => {
                let result = if command == "qbe" {
                    QbeBackend.emit(&program)
                } else {
                    TextBackend.emit(&program)
                };
                match result {
                    Ok(text) => print!("{text}"),
                    Err(error) => {
                        eprintln!("error: {error}");
                        std::process::exit(3);
                    }
                }
            }
            Err(error) => {
                report_diagnostic(&error, &source, path, diagnostic_json);
                std::process::exit(1);
            }
        },
        "rust" => match pipeline(&source) {
            Ok(stmts) => {
                let backend = rust_codegen::RustBackend::new();
                print!("{}", backend.emit_rust(&stmts));
            }
            Err(error) => {
                report_diagnostic(&error, &source, path, diagnostic_json);
                std::process::exit(1);
            }
        },
        "fmt" => {
            if let Err(error) = pipeline(&source) {
                report_diagnostic(&error, &source, path, diagnostic_json);
                std::process::exit(1);
            }
            let mut stdout = io::stdout();
            if let Err(error) = stdout.write_all(source.as_bytes()) {
                eprintln!("error: {error}");
                std::process::exit(2);
            }
        }
        _ => {
            eprintln!("{}", usage());
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::ast::Value;

    #[test]
    fn precedence_and_parentheses() {
        assert_eq!(run("1 + 2 * 3;"), Ok(Value::Int(7)));
        assert_eq!(run("(1 + 2) * 3;"), Ok(Value::Int(9)));
    }

    #[test]
    fn variables_and_unary_minus() {
        assert_eq!(run("let x = 4; -x + 10;"), Ok(Value::Int(6)));
    }

    #[test]
    fn reports_user_errors() {
        assert!(run("unknown;").unwrap_err().contains("undefined variable"));
        assert!(run("1 / 0;").unwrap_err().contains("division by zero"));
        assert!(run("let x: f64 = 1;").unwrap_err().contains("unsupported"));
        assert!(
            run("let x: i32 = 2147483648;")
                .unwrap_err()
                .contains("does not match")
        );
        assert!(
            run("let x: u64 = -1;")
                .unwrap_err()
                .contains("does not match")
        );
        assert!(run("if 1 { 2; }").unwrap_err().contains("boolean"));
    }

    #[test]
    fn evaluates_boolean_logic() {
        assert_eq!(run("true && !false;"), Ok(Value::Bool(true)));
        assert_eq!(
            run("let ready: bool = 2 > 1; ready || false;"),
            Ok(Value::Bool(true))
        );
    }

    #[test]
    fn short_circuits_boolean_logic() {
        assert_eq!(run("false && (1 / 0 == 0);"), Ok(Value::Bool(false)));
        assert_eq!(run("true || (1 / 0 == 0);"), Ok(Value::Bool(true)));
    }

    #[test]
    fn if_else_expressions_work() {
        assert_eq!(
            run("let total = if 1 < 2 { 10 + 5 } else { 33 }; total;"),
            Ok(Value::Int(15))
        );
        assert_eq!(
            run("let value = if false { 1 } else { 2 }; value;"),
            Ok(Value::Int(2))
        );
    }

    #[test]
    fn arrays_support_empty_nested_indexing_and_mutation() {
        assert_eq!(run("let xs: [i32] = []; xs;"), Ok(Value::Array(vec![])));
        assert_eq!(
            run("let xs = [[1, 2], [3, 4]]; xs[1][0];"),
            Ok(Value::Int(3))
        );
        assert_eq!(run("let xs = [1, 2]; xs[0] = 9; xs[0];"), Ok(Value::Int(9)));
    }

    #[test]
    fn arrays_report_type_and_bounds_errors() {
        assert!(
            run("let xs: [i32] = [true];")
                .unwrap_err()
                .contains("type mismatch")
        );
        assert!(
            run("let xs = [1]; xs[2];")
                .unwrap_err()
                .contains("out of bounds")
        );
        assert!(
            run("let xs = [1]; xs[true];")
                .unwrap_err()
                .contains("index")
        );
    }

    #[test]
    fn while_loops_evaluate_conditionally() {
        assert_eq!(run("while false { let x = 5; }"), Ok(Value::Int(0)));
        assert_eq!(
            run("let flag = false; while flag { let x = 1; }"),
            Ok(Value::Int(0))
        );
    }

    #[test]
    fn while_loops_can_update_state() {
        assert_eq!(
            run("let x = 0; while x < 3 { x = x + 1; } x;"),
            Ok(Value::Int(3))
        );
    }

    #[test]
    fn break_and_continue_work_through_nested_control_flow() {
        assert_eq!(
            run(
                "let x = 0; while x < 10 { x = x + 1; if x == 3 { continue; } if x == 6 { break; } } x;"
            ),
            Ok(Value::Int(6))
        );
        assert_eq!(
            run("let x = 0; while x < 3 { if true { x = x + 1; continue; } x = 99; } x;"),
            Ok(Value::Int(3))
        );
        assert_eq!(
            run(
                "let x = 0; while x < 1 { let ignored = if true { break; 1 } else { 2 }; x = 99; } x;"
            ),
            Ok(Value::Int(0))
        );
    }

    #[test]
    fn rejects_loop_control_outside_loops() {
        assert!(
            run("break;")
                .unwrap_err()
                .contains("only be used inside a loop")
        );
        assert!(
            run("if true { continue; }")
                .unwrap_err()
                .contains("only be used inside a loop")
        );
    }

    #[test]
    fn enforces_loop_iteration_limit() {
        let error = run("while true {}").unwrap_err();
        assert!(error.contains("loop iteration limit exceeded"));
    }

    #[test]
    fn enforces_scope_and_assignment_types() {
        assert!(
            run("{ let hidden = 1; }; hidden;")
                .unwrap_err()
                .contains("undefined variable")
        );
        assert!(
            run("let flag: bool = true; flag = 1;")
                .unwrap_err()
                .contains("type mismatch")
        );
    }

    #[test]
    fn accepts_integer_boundaries() {
        assert_eq!(
            run("let low: i32 = -2147483648; let high: i32 = 2147483647; high;"),
            Ok(Value::Int(2147483647))
        );
        assert!(
            run("let low: i32 = -2147483649;")
                .unwrap_err()
                .contains("does not match")
        );
    }

    #[test]
    fn reports_parse_locations() {
        let error = run("let value = 1;\nvalue + ;").unwrap_err();
        assert!(error.contains("parse error"));
        assert!(error.contains("2:"));
    }

    #[test]
    fn evaluates_typed_functions_and_recursion() {
        assert_eq!(
            run("fn add(a: i32, b: i32) -> i32 { a + b } add(2, 3);"),
            Ok(Value::Int(5))
        );
        assert_eq!(
            run(
                "fn factorial(n: i32) -> i32 { if n == 0 { 1 } else { n * factorial(n - 1) } } factorial(5);"
            ),
            Ok(Value::Int(120))
        );
    }

    #[test]
    fn evaluates_distinct_integer_types_and_strings() {
        assert_eq!(run("let value: u64 = u64(4); value;"), Ok(Value::U64(4)));
        assert_eq!(run("\"hello\";"), Ok(Value::String("hello".into())));
        assert_eq!(run("\"hello\" == \"hello\";"), Ok(Value::Bool(true)));
    }

    #[test]
    fn validates_function_arguments_and_conversions() {
        assert!(
            run("fn id(value: i32) -> i32 { value } id(true);")
                .unwrap_err()
                .contains("argument")
        );
        assert!(run("u64(-1);").unwrap_err().contains("cannot convert"));
    }

    #[test]
    fn records_smoke() {
        assert_eq!(
            run("record Point { x: i32, y: i32 } let p: Point = Point { x: 2, y: 3 }; p.x + p.y;"),
            Ok(Value::Int(5))
        );
    }

    #[test]
    fn validates_record_fields() {
        assert!(
            run("record Point { x: i32 } Point { y: 1 };")
                .unwrap_err()
                .contains("unknown field")
        );
        assert!(
            run("record Point { x: i32, y: i32 } Point { x: 1 };")
                .unwrap_err()
                .contains("requires 2 fields")
        );
        assert!(
            run("record Point { x: i32 } Point { x: true };")
                .unwrap_err()
                .contains("wrong type")
        );
        assert!(
            run("record Point { x: i32 } let p = Point { x: 1 }; p.missing;")
                .unwrap_err()
                .contains("unknown field")
        );
    }

    #[test]
    fn evaluates_enum_variants() {
        assert_eq!(
            run(
                "enum Maybe { None, Some { value: i32 } } let value: Maybe = Maybe::Some { value: 7 }; value;"
            ),
            Ok(Value::Enum {
                enum_name: "Maybe".into(),
                variant: "Some".into(),
                fields: [("value".into(), Value::Int(7))].into_iter().collect(),
            })
        );
    }

    #[test]
    fn evaluates_structured_results() {
        assert_eq!(
            run("let success = ok(3); success;"),
            Ok(Value::Result {
                ok: true,
                value: Box::new(Value::Int(3)),
            })
        );
        assert_eq!(
            run("let failure = err(\"bad\"); failure;"),
            Ok(Value::Result {
                ok: false,
                value: Box::new(Value::String("bad".into())),
            })
        );
    }

    #[test]
    fn evaluates_scoped_maps() {
        assert_eq!(
            run(r#"let values = map("answer", 42); map_get(values, "answer");"#),
            Ok(Value::Int(42))
        );
    }

    #[test]
    fn matches_results_and_checks_exhaustiveness() {
        assert_eq!(
            run(r#"let value = ok(7); match value { Ok(number) => number, Err(error) => 0 };"#),
            Ok(Value::Int(7))
        );
        assert!(
            run(r#"let value = ok(7); match value { Ok(number) => number };"#)
                .unwrap_err()
                .contains("non-exhaustive")
        );
    }

    #[test]
    fn propagates_result_errors_with_question_mark() {
        assert_eq!(
            run(r#"fn unwrap() -> Result { ok(4)? } unwrap();"#),
            Ok(Value::Int(4))
        );
        assert_eq!(
            run(r#"fn unwrap() -> Result { err("bad")? } unwrap();"#),
            Ok(Value::Result {
                ok: false,
                value: Box::new(Value::String("bad".into())),
            })
        );
    }
}
