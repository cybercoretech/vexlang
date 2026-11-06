//! Client for the Vex package registry.
//!
//! This crate is the single implementation behind `vexlang login`,
//! `vexlang publish` and `vexlang add`. It talks to the registry's HTTP API,
//! stores credentials in the user's config directory and downloads package
//! tarballs. It uses blocking I/O and is never built for `wasm32`.

use anyhow::{Context, Result, bail};
use reqwest::StatusCode;
use reqwest::blocking::{Client, multipart};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Registry used when neither `--registry` nor `VEX_REGISTRY` is given.
pub const DEFAULT_REGISTRY: &str = "http://localhost:3001";

/// Environment variable that overrides [`DEFAULT_REGISTRY`].
pub const REGISTRY_ENV: &str = "VEX_REGISTRY";

/// Environment variable that overrides where credentials are stored.
pub const CREDENTIALS_ENV: &str = "VEX_CREDENTIALS";

/// Saved login for one registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    pub registry: String,
    /// Access token from a password login, or an API key.
    pub token: String,
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
    access_token: String,
}

/// Registry reply to a successful upload.
#[derive(Debug, Deserialize)]
pub struct PublishUploadResponse {
    pub package: String,
    pub version: String,
    pub checksum: String,
    pub size: i64,
}

/// Registry URL from `VEX_REGISTRY`, falling back to [`DEFAULT_REGISTRY`].
pub fn default_registry() -> String {
    std::env::var(REGISTRY_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_REGISTRY.to_string())
}

/// Strips trailing slashes and rejects anything that is not an http(s) URL.
pub fn normalize_registry(registry: &str) -> Result<String> {
    let base = registry.trim().trim_end_matches('/');
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        bail!("registry must be an http:// or https:// URL, got `{registry}`");
    }
    Ok(base.to_string())
}

/// Checks a package name or version before it is put into a URL or file name.
///
/// Allowed: ASCII letters, digits, `.`, `_`, `-` and `+`, not starting with
/// `.` or `-`, at most 128 characters. This rules out path traversal such as
/// `../x` when `add` writes `NAME-VERSION.tar.gz`.
pub fn validate_component(kind: &str, value: &str) -> Result<()> {
    let ok_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+');
    if value.is_empty()
        || value.len() > 128
        || value.starts_with(['.', '-'])
        || !value.chars().all(ok_char)
    {
        bail!(
            "invalid package {kind} `{value}` (use letters, digits, `.`, `_`, `-`, `+`; \
             must not start with `.` or `-`)"
        );
    }
    Ok(())
}

/// Where credentials live: `VEX_CREDENTIALS`, else
/// `<config dir>/vex/credentials.toml`.
pub fn credentials_path() -> PathBuf {
    if let Some(path) = std::env::var_os(CREDENTIALS_ENV).filter(|p| !p.is_empty()) {
        return PathBuf::from(path);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vex")
        .join("credentials.toml")
}

pub fn load_credentials_from(path: &Path) -> Result<Credentials> {
    let data = fs::read_to_string(path).with_context(|| {
        format!(
            "not logged in (run `vexlang login`): cannot read {}",
            path.display()
        )
    })?;
    toml::from_str(&data).with_context(|| format!("malformed credentials in {}", path.display()))
}

pub fn load_credentials() -> Result<Credentials> {
    load_credentials_from(&credentials_path())
}

/// Writes credentials, readable only by the owner on Unix.
pub fn save_credentials_to(path: &Path, creds: &Credentials) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let body = toml::to_string_pretty(creds)?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        // Create with 0600 so the token is never world-readable, even briefly.
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("cannot write {}", path.display()))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        file.write_all(body.as_bytes())?;
    }
    #[cfg(not(unix))]
    fs::write(path, body).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(())
}

pub fn save_credentials(creds: &Credentials) -> Result<()> {
    save_credentials_to(&credentials_path(), creds)
}

fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(concat!("vex-registry/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(120))
        .build()?)
}

/// Logs in with email and password and saves the returned access token.
pub fn login(registry: &str, email: &str, password: &str) -> Result<Credentials> {
    let base = normalize_registry(registry)?;
    let resp: AuthResponse = client()?
        .post(format!("{base}/api/auth/login"))
        .json(&serde_json::json!({ "email": email, "password": password }))
        .send()
        .with_context(|| format!("cannot reach registry {base}"))?
        .error_for_status()
        .context("login rejected by registry")?
        .json()
        .context("unexpected login response from registry")?;
    let creds = Credentials {
        registry: base,
        token: resp.access_token,
    };
    save_credentials(&creds)?;
    Ok(creds)
}

/// Saves an API key (for CI) without contacting the registry.
pub fn login_api_key(registry: &str, api_key: &str) -> Result<Credentials> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        bail!("API key is empty");
    }
    let creds = Credentials {
        registry: normalize_registry(registry)?,
        token: api_key.to_string(),
    };
    save_credentials(&creds)?;
    Ok(creds)
}

/// Creates the package entry if needed. An existing package (409) is fine.
pub fn ensure_package(creds: &Credentials, name: &str, description: &str) -> Result<()> {
    validate_component("name", name)?;
    let resp = client()?
        .post(format!("{}/api/packages", creds.registry))
        .bearer_auth(&creds.token)
        .json(&serde_json::json!({ "name": name, "description": description }))
        .send()
        .with_context(|| format!("cannot reach registry {}", creds.registry))?;
    let status = resp.status();
    if status.is_success() || status == StatusCode::CONFLICT {
        Ok(())
    } else {
        bail!("registry refused to create package `{name}`: HTTP {status}");
    }
}

/// Uploads a `.tar.gz` as `name@version` and checks the registry's checksum
/// against the local file.
pub fn publish_tarball(
    creds: &Credentials,
    name: &str,
    version: &str,
    tarball: &Path,
) -> Result<PublishUploadResponse> {
    validate_component("name", name)?;
    validate_component("version", version)?;
    if !tarball.is_file() {
        bail!("tarball not found: {}", tarball.display());
    }
    let local = sha256_file(tarball)?;
    let form = multipart::Form::new()
        .text("version", version.to_string())
        .file("file", tarball)
        .with_context(|| format!("cannot read {}", tarball.display()))?;
    let resp: PublishUploadResponse = client()?
        .post(format!("{}/api/packages/{name}/upload", creds.registry))
        .bearer_auth(&creds.token)
        .multipart(form)
        .send()
        .with_context(|| format!("cannot reach registry {}", creds.registry))?
        .error_for_status()
        .with_context(|| format!("upload of {name}@{version} rejected"))?
        .json()
        .context("unexpected upload response from registry")?;
    let digest = |s: &str| s.trim().trim_start_matches("sha256:").to_ascii_lowercase();
    if digest(&resp.checksum) != digest(&local) {
        bail!(
            "checksum mismatch after upload: local {local}, registry {}",
            resp.checksum
        );
    }
    Ok(resp)
}

/// Downloads `name@version` into `out_dir/NAME-VERSION.tar.gz` and returns
/// that path. The file only appears once the download is complete.
pub fn download_package(
    registry: &str,
    token: Option<&str>,
    name: &str,
    version: &str,
    out_dir: &Path,
) -> Result<PathBuf> {
    validate_component("name", name)?;
    validate_component("version", version)?;
    let base = normalize_registry(registry)?;
    let mut req = client()?.get(format!("{base}/api/packages/{name}/{version}/download"));
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let bytes = req
        .send()
        .with_context(|| format!("cannot reach registry {base}"))?
        .error_for_status()
        .with_context(|| format!("cannot download {name}@{version}"))?
        .bytes()?;
    fs::create_dir_all(out_dir).with_context(|| format!("cannot create {}", out_dir.display()))?;
    let dest = out_dir.join(format!("{name}-{version}.tar.gz"));
    let partial = out_dir.join(format!(".{name}-{version}.tar.gz.part"));
    fs::write(&partial, &bytes).with_context(|| format!("cannot write {}", partial.display()))?;
    fs::rename(&partial, &dest).with_context(|| format!("cannot write {}", dest.display()))?;
    Ok(dest)
}

/// `sha256:<hex>` digest of a file, the format the registry reports.
pub fn sha256_file(path: &Path) -> Result<String> {
    let data = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(&data))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vex-registry-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn components_accept_normal_names_and_versions() {
        for ok in [
            "vexstd",
            "my_pkg",
            "my-pkg",
            "0.1.0",
            "1.0.0-alpha.1",
            "1.0+build.5",
        ] {
            validate_component("name", ok).unwrap();
        }
    }

    #[test]
    fn components_reject_traversal_and_junk() {
        for bad in [
            "", "../etc", "a/b", "a\\b", ".hidden", "-flag", "sp ace", "ü",
        ] {
            assert!(validate_component("name", bad).is_err(), "{bad:?} accepted");
        }
        assert!(validate_component("name", &"a".repeat(129)).is_err());
    }

    #[test]
    fn registry_urls_are_normalized() {
        assert_eq!(
            normalize_registry("https://reg.example/").unwrap(),
            "https://reg.example"
        );
        assert_eq!(
            normalize_registry(" http://localhost:3001 ").unwrap(),
            "http://localhost:3001"
        );
        assert!(normalize_registry("ftp://x").is_err());
        assert!(normalize_registry("localhost:3001").is_err());
    }

    #[test]
    fn credentials_round_trip_with_private_permissions() {
        let dir = temp_dir("creds");
        let path = dir.join("nested").join("credentials.toml");
        let creds = Credentials {
            registry: "https://reg.example".into(),
            token: "secret".into(),
        };
        save_credentials_to(&path, &creds).unwrap();
        assert_eq!(load_credentials_from(&path).unwrap(), creds);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_credentials_explain_how_to_log_in() {
        let err = load_credentials_from(Path::new("/nonexistent/vex/credentials.toml"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("vexlang login"), "{err}");
    }

    #[test]
    fn sha256_matches_known_digest() {
        let dir = temp_dir("sha");
        let file = dir.join("abc.txt");
        fs::write(&file, b"abc").unwrap();
        assert_eq!(
            sha256_file(&file).unwrap(),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
