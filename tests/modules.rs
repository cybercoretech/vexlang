//! `use` resolution: inline modules, sibling files, installed packages,
//! exports, and the errors for missing modules, clashes, cycles and
//! side effects.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vexlang-mod-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn vex(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vexlang"))
        .args(args)
        .output()
        .expect("vexlang should run")
}

fn write(dir: &Path, file: &str, body: &str) -> String {
    let path = dir.join(file);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, body).unwrap();
    path.to_string_lossy().into_owned()
}

fn run(path: &str) -> (bool, String) {
    let out = vex(&["run", path]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

#[test]
fn imports_from_inline_modules_and_sibling_files() {
    let dir = temp_dir("basic");
    write(
        &dir,
        "math.vex",
        "fn square(x: i32) -> i32 { x * x }\nexport square;\n",
    );
    let main = write(
        &dir,
        "main.vex",
        "module geo { fn double(x: i32) -> i32 { x + x } }\nuse geo;\nuse math::square;\nsquare(double(3));\n",
    );
    let (ok, text) = run(&main);
    assert!(ok, "{text}");
    assert!(text.contains("Int(36)"), "{text}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn imports_installed_packages_with_custom_lib_path() {
    let dir = temp_dir("pkg");
    write(&dir, "vex.toml", "name = \"app\"\nversion = \"0.1.0\"\n");
    write(
        &dir,
        "vex_modules/tools/vex.toml",
        "name = \"tools\"\nversion = \"1.0.0\"\nlib = \"lib/entry.vex\"\n",
    );
    write(
        &dir,
        "vex_modules/tools/lib/entry.vex",
        "use helpers;\nfn seven() -> i32 { six() + 1 }\n",
    );
    write(
        &dir,
        "vex_modules/tools/lib/helpers.vex",
        "fn six() -> i32 { 6 }\n",
    );
    // Run from a nested directory: the project root is found upwards.
    let main = write(
        &dir,
        "src/bin/main.vex",
        "use tools::seven;\nseven() * 6;\n",
    );
    let (ok, text) = run(&main);
    assert!(ok, "{text}");
    assert!(text.contains("Int(42)"), "{text}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn imported_code_works_in_every_backend() {
    let dir = temp_dir("backends");
    write(&dir, "math.vex", "fn square(x: i32) -> i32 { x * x }\n");
    let main = write(&dir, "main.vex", "use math;\nsquare(5);\n");
    for command in ["check", "ir", "build", "rust", "qbe"] {
        let out = vex(&[command, &main]);
        assert!(
            out.status.success(),
            "{command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let rust = String::from_utf8(vex(&["rust", &main]).stdout).unwrap();
    assert!(
        rust.contains("fn math__square")
            && rust.contains("math__square(5)")
            && !rust.contains("use math"),
        "{rust}"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn reports_module_errors_at_the_use_site() {
    let dir = temp_dir("errors");
    write(
        &dir,
        "math.vex",
        "fn square(x: i32) -> i32 { x * x }\nfn helper() -> i32 { 1 }\nexport square;\n",
    );
    write(&dir, "a.vex", "use b;\nfn fa() -> i32 { 1 }\n");
    write(&dir, "b.vex", "use a;\nfn fb() -> i32 { 2 }\n");
    write(&dir, "noisy.vex", "println(\"side effect\");\n");
    write(&dir, "broken.vex", "fn f() -> i32 { true }\n");
    write(&dir, "other.vex", "fn square(x: i32) -> i32 { x }\n");
    write(&dir, "shape.vex", "record Point { x: i32 }\n");
    let cases = [
        ("use nope;\n1;\n", "E1101", "cannot find module `nope`"),
        ("use math::missing;\n1;\n", "E1101", "has no item `missing`"),
        ("use math::helper;\n1;\n", "E1105", "not exported"),
        ("use a;\n1;\n", "E1103", "import cycle: a -> b -> a"),
        ("use noisy;\n1;\n", "E1102", "may only contain declarations"),
        ("use broken;\n1;\n", "E1102", "error in module `broken`"),
        (
            "use math;\nuse other;\nsquare(2);\n",
            "E1104",
            "`square` is ambiguous: it is imported from module `math` and module `other`",
        ),
        (
            "use shape;\nrecord Point { x: i32 }\n1;\n",
            "E1104",
            "also imported from module `shape`",
        ),
        (
            "math::square(2);\n",
            "E1101",
            "module `math` is not imported here",
        ),
        ("use math;\nmath::helper();\n", "E1105", "not exported"),
        (
            "use math;\nmath::cube(2);\n",
            "E1101",
            "has no function `cube`",
        ),
        (
            "use shape;\nshape::Point(2);\n",
            "E1101",
            "is not a function",
        ),
        (
            "use math;\nhelper();\n",
            "E2001",
            "undefined function `helper`",
        ),
        ("use a::b::c;\n1;\n", "E1101", "nested module paths"),
    ];
    for (i, (program, code, message)) in cases.iter().enumerate() {
        let main = write(&dir, &format!("case{i}.vex"), program);
        let (ok, text) = run(&main);
        assert!(!ok, "{program:?} should fail");
        assert!(
            text.contains(code) && text.contains(message),
            "{program:?}:\n{text}"
        );
    }
    // Machine-readable diagnostics carry the same code.
    let main = write(&dir, "json.vex", "use nope;\n1;\n");
    let out = vex(&["--json", "check", &main]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("\"code\":\"E1101\""));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn modules_are_loaded_once_and_tests_are_not_imported() {
    let dir = temp_dir("once");
    write(
        &dir,
        "base.vex",
        "fn one() -> i32 { 1 }\ntest fn base_test() -> i32 { 0 }\n",
    );
    write(
        &dir,
        "x.vex",
        "use base;\nfn two() -> i32 { one() + one() }\n",
    );
    write(
        &dir,
        "y.vex",
        "use base;\nfn three() -> i32 { one() + two() }\nuse x;\n",
    );
    let main = write(&dir, "main.vex", "use x;\nuse y;\nuse base;\nthree();\n");
    let (ok, text) = run(&main);
    assert!(ok, "{text}");
    assert!(text.contains("Int(3)"), "{text}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn module_functions_are_namespaced() {
    let dir = temp_dir("namespaced");
    write(
        &dir,
        "math.vex",
        "fn helper() -> i32 { 100 }\nfn square(x: i32) -> i32 { x * x + helper() - 100 }\nfn min(a: i32, b: i32) -> i32 { if a < b { a } else { b } }\nexport square;\nexport min;\n",
    );
    write(
        &dir,
        "other.vex",
        "fn min(a: i32, b: i32) -> i32 { 0 - 1 }\n",
    );
    let cases = [
        // Qualified calls, with or without an unqualified import.
        ("use math;\nmath::square(3);\n", "Int(9)"),
        (
            "use math::square;\nmath::min(4, 2) + square(2);\n",
            "Int(6)",
        ),
        // The program's own function wins over the glob import...
        (
            "use math;\nfn min(a: i32, b: i32) -> i32 { 42 }\nmin(1, 2) + math::min(1, 2);\n",
            "Int(43)",
        ),
        // ...and over a module's private helper of the same name.
        (
            "use math;\nfn helper() -> i32 { 7 }\nhelper() + square(2);\n",
            "Int(11)",
        ),
        // Two modules with the same function: fine when qualified.
        (
            "use math;\nuse other;\nmath::min(5, 6) + other::min(5, 6);\n",
            "Int(4)",
        ),
        // `use m::f` picks one of two otherwise ambiguous names.
        ("use math;\nuse other::min;\nmin(5, 6);\n", "Int(-1)"),
    ];
    for (i, (program, expected)) in cases.iter().enumerate() {
        let main = write(&dir, &format!("case{i}.vex"), program);
        let (ok, text) = run(&main);
        assert!(ok && text.contains(expected), "{program:?}:\n{text}");
    }
    let main = write(
        &dir,
        "both.vex",
        "use math;\nuse other;\nmath::min(5, 6) + other::min(5, 6);\n",
    );
    for command in ["check", "ir", "build", "rust", "qbe"] {
        let out = vex(&[command, &main]);
        assert!(
            out.status.success(),
            "{command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let rust = String::from_utf8(vex(&["rust", &main]).stdout).unwrap();
    assert!(
        rust.contains("fn math__min") && rust.contains("fn other__min"),
        "{rust}"
    );
    fs::remove_dir_all(dir).unwrap();
}
