//! End-to-end checks for `vexlang rust`: transpile, compile with `rustc`,
//! run, and compare the output with the interpreter (`vexlang run`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vexlang-rs-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn vex(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_vexlang"))
        .args(args)
        .output()
        .expect("vexlang should run");
    assert!(
        out.status.success(),
        "vexlang {args:?} failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// Interpreter output without the final `Unit` that `run` prints for the
/// last statement.
fn interpreted(source: &Path) -> String {
    let out = vex(&["run", source.to_str().unwrap()]);
    out.strip_suffix("Unit\n").unwrap_or(&out).to_string()
}

fn compiled(dir: &Path, source: &Path) -> String {
    let rust = dir.join("main.rs");
    fs::write(&rust, vex(&["rust", source.to_str().unwrap()])).unwrap();
    let binary = dir.join("main");
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let out = Command::new(rustc)
        .args(["--edition", "2021", "-o"])
        .arg(&binary)
        .arg(&rust)
        .output()
        .expect("rustc should run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "rustc failed:\n{stderr}\n--- generated ---\n{}",
        fs::read_to_string(&rust).unwrap()
    );
    assert!(!stderr.contains("warning"), "rustc warnings:\n{stderr}");
    let run = Command::new(&binary).output().unwrap();
    assert!(run.status.success());
    String::from_utf8(run.stdout).unwrap()
}

fn same_output(tag: &str, files: &[(&str, &str)], expected: &str) {
    let dir = temp_dir(tag);
    for (name, body) in files {
        fs::write(dir.join(name), body).unwrap();
    }
    let main = dir.join(files[0].0);
    let interpreted = interpreted(&main);
    assert_eq!(interpreted, expected, "interpreter output");
    assert_eq!(compiled(&dir, &main), expected, "compiled Rust output");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn results_match_and_try() {
    same_output(
        "result",
        &[(
            "main.vex",
            r#"fn unwrap_or(r: Result, otherwise: i32) -> i32 {
    match r { Ok(value) => value, Err(error) => otherwise }
}
fn is_ok(r: Result) -> bool { match r { Ok(value) => true, Err(error) => false } }
fn half(x: i32) -> Result { if x / 2 * 2 == x { ok(x / 2) } else { err("odd") } }
fn quarter(x: i32) -> Result { let h = half(x)?; half(h) }
println(unwrap_or(ok(5), 0));
println(unwrap_or(err("nope"), 7));
println(is_ok(half(3)));
println(unwrap_or(quarter(8), 0 - 1));
println(unwrap_or(quarter(6), 0 - 1));
let r: Result = half(10);
let m = match r { Ok(v) => v, Err(e) => 0 };
println(m);
"#,
        )],
        "5\n7\nfalse\n2\n-1\n5\n",
    );
}

#[test]
fn strings_and_namespaced_modules() {
    same_output(
        "modules",
        &[
            (
                "main.vex",
                r#"use util;
fn min(a: i32, b: i32) -> i32 { 999 }
fn greet(name: string) -> string { "hello, " + name + "!" }
let who = "Vex";
println(greet(who));
println("n=" + string(util::min(4, 2)) + " own=" + string(min(1, 2)));
println(unwrap_or(util::parse_even(7), 0 - 1));
"#,
            ),
            (
                "util.vex",
                r#"fn min(a: i32, b: i32) -> i32 { if a < b { a } else { b } }
fn parse_even(x: i32) -> Result { if x / 2 * 2 == x { ok(x) } else { err("odd") } }
fn unwrap_or(r: Result, otherwise: i32) -> i32 {
    match r { Ok(value) => value, Err(error) => otherwise }
}
"#,
            ),
        ],
        "hello, Vex!\nn=2 own=999\n-1\n",
    );
}
