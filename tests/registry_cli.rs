//! CLI tests for the package commands against a local static index.
//! No network: published URLs are rewritten to `file://` before installing.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vexlang-pkg-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn vex(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vexlang"))
        .args(args)
        .current_dir(cwd)
        .env_remove("VEX_INDEX")
        .output()
        .expect("vexlang should run")
}

fn ok(cwd: &Path, args: &[&str]) -> String {
    let out = vex(cwd, args);
    assert!(out.status.success(), "{args:?} failed:\n{}", text(&out));
    text(&out)
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// A fresh index dir plus helpers to publish libraries into it.
struct World {
    root: PathBuf,
    index: PathBuf,
}

impl World {
    fn new(tag: &str) -> Self {
        let root = temp_dir(tag);
        let index = root.join("index");
        fs::create_dir_all(&index).unwrap();
        fs::write(index.join("index.json"), "{\"schema\":1,\"packages\":{}}\n").unwrap();
        World { root, index }
    }

    fn index_arg(&self) -> &str {
        self.index.to_str().unwrap()
    }

    /// Creates, packages and publishes library `name@version` whose
    /// `lib.vex` is `code`, then points its index entry at the local file.
    fn publish(&self, name: &str, version: &str, deps: &[(&str, &str)], code: &str) {
        let dir = self.root.join(format!("{name}-{version}-src"));
        fs::create_dir_all(dir.join("src")).unwrap();
        let mut manifest = format!(
            "[project]\nname = \"{name}\"\nversion = \"{version}\"\ndescription = \"The {name} library\"\n\n[dependencies]\n"
        );
        for (d, v) in deps {
            manifest.push_str(&format!("{d} = \"{v}\"\n"));
        }
        fs::write(dir.join("vex.toml"), manifest).unwrap();
        fs::write(dir.join("src/lib.vex"), code).unwrap();
        ok(&dir, &["package"]);
        let placeholder = format!("https://example.invalid/{name}-{version}.tar.gz");
        ok(
            &dir,
            &[
                "publish",
                "--index",
                self.index_arg(),
                "--url",
                &placeholder,
            ],
        );
        let tarball = dir.join(format!("dist/{name}-{version}.tar.gz"));
        let file = self.index.join("index.json");
        let json = fs::read_to_string(&file)
            .unwrap()
            .replace(&placeholder, &format!("file://{}", tarball.display()));
        fs::write(file, json).unwrap();
    }

    fn app(&self) -> PathBuf {
        ok(&self.root, &["new", "app"]);
        self.root.join("app")
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn new_scaffolds_programs_and_libraries() {
    let w = World::new("new");
    ok(&w.root, &["new", "prog"]);
    ok(&w.root, &["new", "lib", "--lib"]);
    assert!(w.root.join("prog/src/main.vex").is_file());
    assert!(w.root.join("lib/src/lib.vex").is_file());
    assert!(
        fs::read_to_string(w.root.join("lib/.gitignore"))
            .unwrap()
            .contains("vex_modules/")
    );
    assert_eq!(
        ok(&w.root.join("prog"), &["run", "src/main.vex"])
            .lines()
            .next(),
        Some("hello from Vex")
    );
    assert_eq!(vex(&w.root, &["new", "prog"]).status.code(), Some(1));
    assert_eq!(vex(&w.root, &["new", "../evil"]).status.code(), Some(1));
}

#[test]
fn add_installs_transitive_dependencies_usable_from_code() {
    let w = World::new("transitive");
    w.publish("base", "1.0.0", &[], "fn base_value() -> i32 { 20 }\n");
    w.publish(
        "mid",
        "0.1.0",
        &[("base", "1.0.0")],
        "use base;\nfn answer() -> i32 { base_value() + 22 }\nexport answer;\n",
    );
    let app = w.app();
    let out = ok(&app, &["add", "mid", "--index", w.index_arg()]);
    assert!(
        out.contains("installed base@1.0.0") && out.contains("installed mid@0.1.0"),
        "{out}"
    );
    assert!(
        fs::read_to_string(app.join("vex.toml"))
            .unwrap()
            .contains("mid = \"0.1.0\"")
    );
    assert!(app.join("vex.lock").is_file());

    fs::write(
        app.join("src/main.vex"),
        "use mid::answer;\nprintln(answer());\n",
    )
    .unwrap();
    assert_eq!(
        ok(&app, &["run", "src/main.vex"]).lines().next(),
        Some("42")
    );

    // Reinstall from lock + cache with no index at all.
    fs::remove_dir_all(app.join("vex_modules/mid")).unwrap();
    ok(
        &app,
        &[
            "install",
            "--locked",
            "--offline",
            "--index",
            "/nonexistent",
        ],
    );
    assert_eq!(
        ok(&app, &["run", "src/main.vex"]).lines().next(),
        Some("42")
    );

    let out = ok(&app, &["remove", "mid", "--index", w.index_arg()]);
    assert!(
        out.contains("removed   mid") && out.contains("removed   base"),
        "{out}"
    );
    assert!(!app.join("vex_modules/base").exists());
    let out = vex(&app, &["check", "src/main.vex"]);
    assert!(
        text(&out).contains("cannot find module `mid`"),
        "{}",
        text(&out)
    );
}

#[test]
fn add_failure_leaves_manifest_untouched() {
    let w = World::new("rollback");
    let app = w.app();
    let before = fs::read_to_string(app.join("vex.toml")).unwrap();
    let out = vex(&app, &["add", "ghost@1.0.0", "--index", w.index_arg()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("not in the index"), "{}", text(&out));
    assert_eq!(fs::read_to_string(app.join("vex.toml")).unwrap(), before);
}

#[test]
fn install_locked_detects_stale_lock() {
    let w = World::new("stale");
    w.publish("base", "1.0.0", &[], "fn b() -> i32 { 1 }\n");
    let app = w.app();
    let manifest = app.join("vex.toml");
    let text_before = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest, format!("{text_before}base = \"1.0.0\"\n")).unwrap();
    let out = vex(&app, &["install", "--locked", "--index", w.index_arg()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("out of date"), "{}", text(&out));
    ok(&app, &["install", "--index", w.index_arg()]);
    ok(&app, &["install", "--locked", "--index", w.index_arg()]);
}

#[test]
fn search_info_yank_and_fetch() {
    let w = World::new("browse");
    w.publish("strs", "1.0.0", &[], "fn s() -> i32 { 1 }\n");
    w.publish("strs", "1.1.0", &[], "fn s() -> i32 { 2 }\n");
    let idx = w.index_arg();
    assert!(
        ok(&w.root, &["search", "STRS", "--index", idx]).contains("strs 1.1.0  The strs library")
    );
    assert!(ok(&w.root, &["search", "zzz", "--index", idx]).contains("no packages match"));
    let info = ok(&w.root, &["info", "strs", "--index", idx]);
    assert!(
        info.find("1.1.0").unwrap() < info.find("1.0.0").unwrap(),
        "{info}"
    );

    ok(&w.root, &["yank", "strs@1.1.0", "--index", idx]);
    assert!(ok(&w.root, &["info", "strs", "--index", idx]).contains("1.1.0 (yanked)"));
    let app = w.app();
    let out = ok(&app, &["add", "strs", "--index", idx]);
    assert!(
        out.contains("installed strs@1.0.0"),
        "latest non-yanked: {out}"
    );
    assert_eq!(
        vex(&w.root, &["yank", "strs@1.1.0", "--index", idx])
            .status
            .code(),
        Some(1)
    );

    let out = vex(&w.root, &["fetch", "strs@1.1.0", "--index", idx]);
    assert!(text(&out).contains("yanked"), "{}", text(&out));
    ok(
        &w.root,
        &["fetch", "strs@1.0.0", "--index", idx, "--out", "vendor"],
    );
    assert!(w.root.join("vendor/strs-1.0.0.tar.gz").is_file());
}

#[test]
fn publish_is_immutable_and_https_only() {
    let w = World::new("publish");
    w.publish("p", "1.0.0", &[], "fn p() -> i32 { 1 }\n");
    let src = w.root.join("p-1.0.0-src");
    let out = vex(
        &src,
        &["publish", "--index", w.index_arg(), "--url", "https://x/y"],
    );
    assert!(text(&out).contains("immutable"), "{}", text(&out));
    let out = vex(
        &src,
        &["publish", "--index", w.index_arg(), "--url", "http://x/y"],
    );
    assert!(text(&out).contains("https://"), "{}", text(&out));
}

#[test]
fn rejects_path_traversal_and_usage_errors() {
    let w = World::new("usage");
    let app = w.app();
    let out = vex(&app, &["add", "../evil@1.0.0", "--index", w.index_arg()]);
    assert!(
        text(&out).contains("invalid package name"),
        "{}",
        text(&out)
    );
    for args in [
        &["add"][..],
        &["fetch", "demo"][..],
        &["publish", "--url", "https://x/y"][..],
        &["package", "--bogus"][..],
        &["install", "extra"][..],
        &["yank", "p@1"][..],
        &["info"][..],
    ] {
        let out = vex(&app, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", text(&out));
        assert!(text(&out).contains("vexlang install"), "usage text missing");
    }
    let out = vex(&w.root, &["install"]);
    assert!(text(&out).contains("vexlang new"), "{}", text(&out));
}

#[test]
fn ranges_add_and_update() {
    let w = World::new("ranges");
    w.publish("lib1", "1.0.0", &[], "fn v() -> i32 { 10 }\n");
    w.publish("lib1", "2.0.0", &[], "fn v() -> i32 { 20 }\n");
    let idx = w.index_arg();
    let app = w.app();
    let out = ok(&app, &["add", "lib1@^1", "--index", idx]);
    assert!(
        out.contains("installed lib1@1.0.0") && out.contains("added lib1 = \"^1\""),
        "{out}"
    );
    w.publish("lib1", "1.4.0", &[], "fn v() -> i32 { 14 }\n");
    // install keeps the lock; update moves within the range, not to 2.0.0.
    let out = ok(&app, &["install", "--index", idx]);
    assert!(!out.contains("updated"), "{out}");
    let out = ok(&app, &["update", "--index", idx]);
    assert!(out.contains("updated   lib1 1.0.0 -> 1.4.0"), "{out}");
    fs::write(app.join("src/main.vex"), "use lib1;\nprintln(v());\n").unwrap();
    assert_eq!(
        ok(&app, &["run", "src/main.vex"]).lines().next(),
        Some("14")
    );
    ok(&app, &["install", "--locked", "--index", idx]);

    let out = vex(&app, &["update", "ghost", "--index", idx]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("not a dependency"), "{}", text(&out));
    let out = vex(&app, &["add", "lib1@>>1", "--index", idx]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("invalid"), "{}", text(&out));
    let out = vex(&app, &["add", "lib1@^5", "--index", idx]);
    assert!(
        text(&out).contains("no version of `lib1` satisfies"),
        "{}",
        text(&out)
    );
    assert!(
        fs::read_to_string(app.join("vex.toml"))
            .unwrap()
            .contains("lib1 = \"^1\"")
    );
}
