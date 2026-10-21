use std::fmt::Write as _;
use std::process::{Command, Stdio};
use std::time::Instant;

fn vex(source: &str, command: &str) -> std::process::Output {
    vex_args(source, &[command, "-"])
}

fn vex_args(source: &str, args: &[&str]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_vexlang"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("vexlang binary should start");
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("stdin should be available")
            .write_all(source.as_bytes())
            .expect("source should be accepted");
    }
    child.wait_with_output().expect("vexlang should exit")
}

fn repl(input: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_vexlang"))
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("vexlang REPL should start");
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("REPL stdin should be available")
            .write_all(input.as_bytes())
            .expect("REPL input should be accepted");
    }
    child.wait_with_output().expect("REPL should exit")
}

#[test]
fn repl_runs_side_effects_once_and_keeps_session_state() {
    let output =
        repl("let counter = 0;\nprintln(\"booted\");\ncounter = counter + 1;\ncounter;\n:q\n");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert_eq!(stdout.matches("booted").count(), 1, "{stdout}");
    assert!(stdout.contains("Int(1)"), "{stdout}");
}

#[test]
fn repl_keeps_functions_and_rolls_back_failed_inputs() {
    let output = repl(
        "let base = 20;\nfn add_base(n: i32) -> i32 { n + 20 }\nadd_base(base);\n1 / 0;\nbase;\n:q\n",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success());
    assert!(stdout.contains("Int(40)"), "{stdout}");
    assert!(stdout.contains("Int(20)"), "{stdout}");
    assert!(stderr.contains("division by zero"), "{stderr}");
}

#[test]
fn cli_version_matches_package_metadata() {
    let output = Command::new(env!("CARGO_BIN_EXE_vexlang"))
        .arg("--version")
        .output()
        .expect("vexlang binary should start");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("vexlang {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn compatibility_corpus_runs_and_lowers() {
    let cases = [
        ("1 + 2 * 3;", "Int(7)"),
        ("let x: i32 = 0; while x < 3 { x = x + 1; } x;", "Int(3)"),
        (
            "fn add(a: i32, b: i32) -> i32 { a + b } add(2, 5);",
            "Int(7)",
        ),
        ("let value: u64 = u64(4); value == u64(4);", "Bool(true)"),
        (
            "record Point { x: i32, y: i32 } let p: Point = Point { x: 2, y: 3 }; p.x + p.y;",
            "Int(5)",
        ),
    ];

    for (source, expected) in cases {
        let run = vex(source, "run");
        assert!(
            run.status.success(),
            "run failed for {source:?}: {:?}",
            run.stderr
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), expected);

        let ir = vex(source, "build");
        assert!(
            ir.status.success(),
            "lowering failed for {source:?}: {:?}",
            ir.stderr
        );
        let text = String::from_utf8_lossy(&ir.stdout);
        assert!(!text.trim().is_empty(), "empty IR for {source:?}");
    }
}

#[test]
fn diagnostic_fixture_is_stable() {
    let output = vex("let value = missing;\n", "check");
    assert!(!output.status.success());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        diagnostic,
        r#"-:1:13: error[E2001]: undefined variable `missing`
   1 | let value = missing;
     |             ^^^^^^^
   1 | let value = missing;
     |             ------- undefined name referenced here
     = help: declare the variable with `let` before using it
"#
    );
}

#[test]
fn check_reports_multiple_undefined_variables() {
    let output = vex("missing_one;\nmissing_two;\nmissing_fn();\n", "check");
    assert!(!output.status.success());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("undefined variable `missing_one`"));
    assert!(diagnostic.contains("undefined variable `missing_two`"));
    assert!(diagnostic.contains("undefined function `missing_fn`"));

    let output = vex_args(
        "missing_one;\nmissing_two;\nmissing_fn();\n",
        &["--json", "check", "-"],
    );
    assert!(!output.status.success());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.trim_start().starts_with('['));
    assert!(diagnostic.contains("undefined variable `missing_one`"));
    assert!(diagnostic.contains("undefined variable `missing_two`"));
    assert!(diagnostic.contains("undefined function `missing_fn`"));
}

#[test]
fn check_reports_multiple_parse_errors_when_recoverable() {
    let output = vex("let = ;\nfn (\n", "check");
    assert!(!output.status.success());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(
        diagnostic.matches("error[E1001]").count() >= 2,
        "{diagnostic}"
    );

    let output = vex_args("let = ;\nfn (\n", &["--json", "check", "-"]);
    assert!(!output.status.success());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.trim_start().starts_with('['));
    assert!(
        diagnostic.matches("\"code\":\"E1001\"").count() >= 2,
        "{diagnostic}"
    );
}

#[test]
fn diagnostics_can_label_declaration_sites() {
    let output = vex("let x: bool = 1;\n", "check");
    assert!(!output.status.success());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("type mismatch for `x`"));
    assert!(diagnostic.contains("declared here"));
}

#[test]
fn json_diagnostic_fixture_is_stable() {
    let output = vex_args("let value = missing;\n", &["--json", "check", "-"]);
    assert!(!output.status.success());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("\"severity\":\"error\""));
    assert!(diagnostic.contains("\"code\":\"E2001\""));
    assert!(diagnostic.contains("\"message\":\"undefined variable `missing`\""));
    assert!(diagnostic.contains("\"line\":1"));
    assert!(diagnostic.contains("\"column\":13"));
    assert!(diagnostic.contains("\"labels\":[{"));
    assert!(diagnostic.contains("undefined name referenced here"));
    assert!(
        diagnostic.contains("\"suggestion\":\"declare the variable with `let` before using it\"")
    );
}

#[test]
fn generated_arithmetic_corpus_is_consistent() {
    for n in 0..64 {
        let source = format!("({n} + 2) * 3 - {n};");
        let output = vex(&source, "run");
        assert!(output.status.success(), "generated source failed: {source}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            format!("Int({})", n * 2 + 6)
        );
    }
}

#[test]
fn qbe_fixture_is_stable() {
    let source = include_str!("fixtures/qbe_scalar.vex");
    let expected = include_str!("fixtures/qbe_scalar.expected.ssa");
    let output = vex(source, "qbe");
    assert!(output.status.success(), "qbe failed: {:?}", output.stderr);
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn textual_ir_is_deterministic_and_not_a_native_artifact() {
    let source = "let answer = 6 * 7; answer;";
    let first = vex(source, "build");
    let second = vex(source, "ir");
    assert!(first.status.success());
    assert!(second.status.success());
    assert_eq!(first.stdout, second.stdout);
    let ir = String::from_utf8_lossy(&first.stdout);
    assert!(ir.contains("i32"));
    assert!(!ir.contains("\u{7f}"));
}

#[test]
fn target_commands_emit_scalar_artifacts_and_reject_unsupported() {
    let scalar_source = "let x: i32 = 1; while x < 3 { x = x + 1; } x;";
    let output = vex(scalar_source, "target");
    assert!(
        output.status.success(),
        "target failed: {:?}",
        output.stderr
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("target vex-scalar-v1\nentry vex_main\n"));

    let qbe = vex("fn add(a: i32, b: i32) -> i32 { a + b } add(2, 3);", "qbe");
    assert!(qbe.status.success(), "qbe failed: {:?}", qbe.stderr);
    let stdout = String::from_utf8_lossy(&qbe.stdout);
    assert!(stdout.starts_with("# QBE intermediate language emitted by Vex"));
    assert!(stdout.contains("function w $add(w %p_a, w %p_b)"));
    assert!(stdout.contains("export function w $vex_main()"));

    let non_scalar_source = "let xs = [1, 2]; xs;";
    for command in ["target", "qbe"] {
        let output = vex(non_scalar_source, command);
        assert_eq!(output.status.code(), Some(3));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("BE002"));
    }
}

#[test]
#[ignore = "manual performance smoke test; run with cargo test --test release_readiness -- --ignored --nocapture"]
fn performance_smoke() {
    let mut source = String::new();
    for n in 0..2_000 {
        writeln!(&mut source, "{n};").expect("writing to a string cannot fail");
    }
    let start = Instant::now();
    let output = vex(&source, "check");
    assert!(output.status.success());
    eprintln!("checked 2,000 statements in {:?}", start.elapsed());
}
