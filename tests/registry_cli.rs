//! CLI tests for `vexlang package | publish | add` against a local static
//! index. No network: tarball URLs are rewritten to `file://` for `add`.

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

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn project(dir: &Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("vex.toml"),
        "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(dir.join("src/main.vex"), "println(\"hi\");\n").unwrap();
}

#[test]
fn package_publish_add_round_trip() {
    let dir = temp_dir("roundtrip");
    let proj = dir.join("demo");
    let index = dir.join("index");
    fs::create_dir_all(&index).unwrap();
    project(&proj);

    let out = vex(&proj, &["package"]);
    assert!(out.status.success(), "{}", text(&out));
    let tarball = proj.join("dist/demo-0.1.0.tar.gz");
    assert!(tarball.is_file());
    assert!(text(&out).contains("src/main.vex"));

    let out = vex(
        &proj,
        &[
            "publish",
            "--index",
            index.to_str().unwrap(),
            "--url",
            "https://example.invalid/demo-0.1.0.tar.gz",
            "--description",
            "Demo",
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("open a pull request"));

    // Publishing the same version again is refused.
    let out = vex(
        &proj,
        &[
            "publish",
            "--index",
            index.to_str().unwrap(),
            "--url",
            "https://x/y",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("immutable"), "{}", text(&out));

    // Serve the tarball from disk for `add`.
    let index_file = index.join("index.json");
    let json = fs::read_to_string(&index_file).unwrap().replace(
        "https://example.invalid/demo-0.1.0.tar.gz",
        &format!("file://{}", tarball.display()),
    );
    fs::write(&index_file, json).unwrap();

    let vendor = dir.join("vendor");
    let out = vex(
        &dir,
        &[
            "add",
            "demo@0.1.0",
            "--index",
            index.to_str().unwrap(),
            "--out",
            vendor.to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        fs::read(vendor.join("demo-0.1.0.tar.gz")).unwrap(),
        fs::read(&tarball).unwrap()
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn publish_rejects_plain_http_urls() {
    let dir = temp_dir("http");
    project(&dir);
    assert!(vex(&dir, &["package"]).status.success());
    let out = vex(
        &dir,
        &[
            "publish",
            "--index",
            dir.to_str().unwrap(),
            "--url",
            "http://x/t.tar.gz",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("https://"), "{}", text(&out));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn add_rejects_path_traversal_and_unknown_packages() {
    let dir = temp_dir("add");
    fs::write(dir.join("index.json"), r#"{"schema":1,"packages":{}}"#).unwrap();
    let index = dir.to_str().unwrap();

    let out = vex(&dir, &["add", "../evil@1.0.0", "--index", index]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out).contains("invalid package name"),
        "{}",
        text(&out)
    );

    let out = vex(&dir, &["add", "nope@1.0.0", "--index", index]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("not in the index"), "{}", text(&out));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn usage_errors_exit_2() {
    let dir = temp_dir("usage");
    project(&dir);
    for args in [
        &["add"][..],
        &["add", "demo"][..],
        &["publish", "--url", "https://x/y"][..],
        &["package", "--bogus"][..],
        &["add", "demo@1", "--out"][..],
    ] {
        let out = vex(&dir, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", text(&out));
        assert!(text(&out).contains("vexlang package"), "usage text missing");
    }
    fs::remove_dir_all(dir).unwrap();
}
