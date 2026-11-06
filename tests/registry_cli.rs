//! CLI tests for `vexlang login | publish | add`. None of these need a live
//! registry: they cover argument handling, local credential storage, input
//! validation and the "registry unreachable" path (127.0.0.1:9, discard port).

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const DEAD_REGISTRY: &str = "http://127.0.0.1:9";

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vexlang-registry-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn vex(creds: &PathBuf, args: &[&str], envs: &[(&str, &str)], stdin: &str) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vexlang"));
    cmd.args(args)
        .env("VEX_CREDENTIALS", creds)
        .env_remove("VEX_REGISTRY")
        .env_remove("VEX_REGISTRY_PASSWORD")
        .env_remove("VEX_REGISTRY_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in envs {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn().expect("vexlang should start");
    {
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
    }
    child.wait_with_output().expect("vexlang should exit")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn api_key_login_saves_private_credentials() {
    let dir = temp_dir("key");
    let creds = dir.join("credentials.toml");
    let out = vex(
        &creds,
        &["login", "--key-stdin", "--registry", "https://reg.example/"],
        &[],
        "k3y\n",
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let saved = std::fs::read_to_string(&creds).unwrap();
    assert!(
        saved.contains("registry = \"https://reg.example\""),
        "{saved}"
    );
    assert!(saved.contains("token = \"k3y\""), "{saved}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&creds).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn api_key_login_reads_token_from_env() {
    let dir = temp_dir("env");
    let creds = dir.join("credentials.toml");
    let out = vex(
        &creds,
        &["login"],
        &[("VEX_REGISTRY_TOKEN", "from-env")],
        "",
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        std::fs::read_to_string(&creds)
            .unwrap()
            .contains("from-env")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn password_flag_is_refused() {
    let dir = temp_dir("pw");
    let creds = dir.join("credentials.toml");
    let out = vex(
        &creds,
        &["login", "--email", "a@b.c", "--password", "hunter2"],
        &[],
        "",
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("--password-stdin"),
        "{}",
        stderr(&out)
    );
    assert!(!creds.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn email_login_without_password_source_is_a_usage_error() {
    let dir = temp_dir("nopw");
    let out = vex(&dir.join("c.toml"), &["login", "--email", "a@b.c"], &[], "");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("VEX_REGISTRY_PASSWORD"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unreachable_registry_fails_cleanly() {
    let dir = temp_dir("dead");
    let creds = dir.join("c.toml");
    let out = vex(
        &creds,
        &[
            "login",
            "--email",
            "a@b.c",
            "--password-stdin",
            "--registry",
            DEAD_REGISTRY,
        ],
        &[],
        "pw\n",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("cannot reach registry"),
        "{}",
        stderr(&out)
    );
    assert!(!creds.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn publish_without_login_points_at_login() {
    let dir = temp_dir("nologin");
    let out = vex(
        &dir.join("missing.toml"),
        &[
            "publish",
            "--name",
            "pkg",
            "--version",
            "0.1.0",
            "--file",
            "x.tar.gz",
        ],
        &[],
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("vexlang login"), "{}", stderr(&out));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn add_rejects_path_traversal_before_any_request() {
    let dir = temp_dir("traversal");
    let out = vex(
        &dir.join("missing.toml"),
        &[
            "add",
            "--name",
            "../evil",
            "--version",
            "1.0.0",
            "--out",
            dir.to_str().unwrap(),
            "--registry",
            DEAD_REGISTRY,
        ],
        &[],
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("invalid package name"),
        "{}",
        stderr(&out)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_and_unknown_arguments_are_usage_errors() {
    let dir = temp_dir("usage");
    let creds = dir.join("c.toml");
    for args in [
        &["add", "--name", "pkg"][..],
        &["publish", "--name", "pkg", "--version"][..],
        &["add", "--name", "pkg", "--version", "1", "--bogus"][..],
    ] {
        let out = vex(&creds, args, &[], "");
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        assert!(stderr(&out).contains("vexlang login"), "usage text missing");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
