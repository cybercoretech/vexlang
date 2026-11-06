//! Client for the Vex static package index.
//!
//! There is no registry server. The index is one JSON file in a Git
//! repository, and package tarballs are hosted elsewhere (normally GitHub
//! release assets). Each version entry records the tarball URL, SHA-256 and
//! size:
//!
//! ```json
//! { "schema": 1,
//!   "packages": { "mypkg": { "description": "…",
//!     "versions": { "0.1.0": { "url": "https://…/mypkg-0.1.0.tar.gz",
//!                              "sha256": "…", "size": 1234 } } } } }
//! ```
//!
//! - `package` builds a reproducible `NAME-VERSION.tar.gz` from a project.
//! - `publish` adds a version to a local checkout of the index, which then
//!   goes up as a pull request. Published versions are immutable.
//! - `add` resolves a version from the index, downloads it and verifies it.
//!
//! This crate is native-only (blocking I/O); the vexlang wasm32 build never
//! links it.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Index used when neither `--index` nor `VEX_INDEX` is given.
pub const DEFAULT_INDEX: &str =
    "https://raw.githubusercontent.com/cybercoretech/vex-index/main/index.json";

/// Environment variable that overrides [`DEFAULT_INDEX`].
pub const INDEX_ENV: &str = "VEX_INDEX";

/// File name of the index inside an index checkout.
pub const INDEX_FILE: &str = "index.json";

/// Current index format.
pub const SCHEMA: u32 = 1;

/// Largest tarball `add` will download or `publish` will register.
pub const MAX_TARBALL_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub schema: u32,
    #[serde(default)]
    pub packages: BTreeMap<String, PackageEntry>,
}

impl Default for Index {
    fn default() -> Self {
        Index {
            schema: SCHEMA,
            packages: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageEntry {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub versions: BTreeMap<String, VersionEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionEntry {
    pub url: String,
    /// Lowercase hex SHA-256 of the tarball.
    pub sha256: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub yanked: bool,
}

/// Index location from `VEX_INDEX`, falling back to [`DEFAULT_INDEX`].
pub fn default_index() -> String {
    std::env::var(INDEX_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_INDEX.to_string())
}

/// Checks a package name or version before it is used in a URL or file name.
///
/// Allowed: ASCII letters, digits, `.`, `_`, `-` and `+`, not starting with
/// `.` or `-`, at most 128 characters. This rules out path traversal such as
/// `../x` when a tarball is written as `NAME-VERSION.tar.gz`.
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

/// Lowercase hex SHA-256 of a byte slice.
pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let data = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(sha256_hex(&data))
}

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("vex-registry/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(120))
        .build()?)
}

/// Reads `location`, which is an `https://`/`http://` URL, a `file://` URL,
/// or a local path. At most `limit` bytes are accepted.
pub fn fetch(location: &str, limit: u64) -> Result<Vec<u8>> {
    let bytes = if location.starts_with("https://") || location.starts_with("http://") {
        let resp = client()?
            .get(location)
            .send()
            .with_context(|| format!("cannot reach {location}"))?
            .error_for_status()
            .with_context(|| format!("cannot fetch {location}"))?;
        if resp.content_length().is_some_and(|len| len > limit) {
            bail!("{location} is larger than {limit} bytes");
        }
        let mut body = Vec::new();
        resp.take(limit + 1)
            .read_to_end(&mut body)
            .with_context(|| format!("cannot read {location}"))?;
        body
    } else {
        let path = location.strip_prefix("file://").unwrap_or(location);
        let mut body = Vec::new();
        fs::File::open(path)
            .with_context(|| format!("cannot open {path}"))?
            .take(limit + 1)
            .read_to_end(&mut body)
            .with_context(|| format!("cannot read {path}"))?;
        body
    };
    if bytes.len() as u64 > limit {
        bail!("{location} is larger than {limit} bytes");
    }
    Ok(bytes)
}

/// Loads an index from a URL, an `index.json` path, or an index checkout
/// directory.
pub fn load_index(location: &str) -> Result<Index> {
    let path = Path::new(location);
    let location = if path.is_dir() {
        path.join(INDEX_FILE).to_string_lossy().into_owned()
    } else {
        location.to_string()
    };
    let bytes = fetch(&location, 16 * 1024 * 1024)?;
    let index: Index = serde_json::from_slice(&bytes)
        .with_context(|| format!("{location} is not a valid Vex index"))?;
    if index.schema != SCHEMA {
        bail!(
            "{location} uses index schema {} but this vexlang understands schema {SCHEMA}",
            index.schema
        );
    }
    Ok(index)
}

impl Index {
    /// The entry for `name@version`, refusing yanked versions.
    pub fn resolve(&self, name: &str, version: &str) -> Result<&VersionEntry> {
        validate_component("name", name)?;
        validate_component("version", version)?;
        let package = self
            .packages
            .get(name)
            .with_context(|| format!("package `{name}` is not in the index"))?;
        let entry = package.versions.get(version).with_context(|| {
            let known: Vec<&str> = package.versions.keys().map(String::as_str).collect();
            format!(
                "`{name}` has no version `{version}` (available: {})",
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            )
        })?;
        if entry.yanked {
            bail!("{name}@{version} has been yanked");
        }
        Ok(entry)
    }
}

/// Downloads `name@version` into `out_dir/NAME-VERSION.tar.gz`, verifying the
/// size and SHA-256 from the index. Nothing is written unless both match.
pub fn add(index: &Index, name: &str, version: &str, out_dir: &Path) -> Result<PathBuf> {
    let entry = index.resolve(name, version)?;
    let bytes = fetch(&entry.url, MAX_TARBALL_BYTES.min(entry.size))?;
    if bytes.len() as u64 != entry.size {
        bail!(
            "{name}@{version}: downloaded {} bytes, index says {}",
            bytes.len(),
            entry.size
        );
    }
    let actual = sha256_hex(&bytes);
    if !actual.eq_ignore_ascii_case(&entry.sha256) {
        bail!(
            "{name}@{version}: SHA-256 mismatch (index {}, downloaded {actual})",
            entry.sha256
        );
    }
    fs::create_dir_all(out_dir).with_context(|| format!("cannot create {}", out_dir.display()))?;
    let dest = out_dir.join(format!("{name}-{version}.tar.gz"));
    let partial = out_dir.join(format!(".{name}-{version}.tar.gz.part"));
    fs::write(&partial, &bytes).with_context(|| format!("cannot write {}", partial.display()))?;
    fs::rename(&partial, &dest).with_context(|| format!("cannot write {}", dest.display()))?;
    Ok(dest)
}

/// What [`publish`] recorded.
#[derive(Debug)]
pub struct Published {
    pub index_file: PathBuf,
    pub entry: VersionEntry,
}

/// Adds `name@version` (served from `url`) to the index checkout at
/// `index_dir`, creating `index.json` if it does not exist yet.
///
/// Published versions are immutable: an existing `name@version` is an error.
/// `url` must be `https://` so that consumers never fetch over plain HTTP.
pub fn publish(
    index_dir: &Path,
    name: &str,
    version: &str,
    tarball: &Path,
    url: &str,
    description: Option<&str>,
) -> Result<Published> {
    validate_component("name", name)?;
    validate_component("version", version)?;
    if !url.starts_with("https://") {
        bail!("tarball URL must start with https://, got `{url}`");
    }
    if !index_dir.is_dir() {
        bail!(
            "{} is not a directory (pass a checkout of the index repository)",
            index_dir.display()
        );
    }
    let size = fs::metadata(tarball)
        .with_context(|| format!("tarball not found: {}", tarball.display()))?
        .len();
    if size > MAX_TARBALL_BYTES {
        bail!(
            "{} is {size} bytes; the limit is {MAX_TARBALL_BYTES}",
            tarball.display()
        );
    }
    let index_file = index_dir.join(INDEX_FILE);
    let mut index = if index_file.exists() {
        load_index(&index_file.to_string_lossy())?
    } else {
        Index::default()
    };
    let package = index.packages.entry(name.to_string()).or_default();
    if package.versions.contains_key(version) {
        bail!("{name}@{version} is already in the index; published versions are immutable");
    }
    if let Some(description) = description {
        package.description = description.to_string();
    }
    let entry = VersionEntry {
        url: url.to_string(),
        sha256: sha256_file(tarball)?,
        size,
        yanked: false,
    };
    package.versions.insert(version.to_string(), entry.clone());

    let mut json = serde_json::to_string_pretty(&index)?;
    json.push('\n');
    let partial = index_dir.join(".index.json.part");
    fs::write(&partial, json).with_context(|| format!("cannot write {}", partial.display()))?;
    fs::rename(&partial, &index_file)
        .with_context(|| format!("cannot write {}", index_file.display()))?;
    Ok(Published { index_file, entry })
}

/// Files `package` puts in a tarball: the manifest, README/LICENSE files and
/// every `.vex` source, skipping hidden directories, `target/` and `dist/`.
pub fn package_files(project_dir: &Path, manifest: &Path) -> Result<Vec<PathBuf>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let file_type = entry.file_type()?;
            if name.starts_with('.') || file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if dir == root && (name == "target" || name == "dist") {
                    continue;
                }
                walk(root, &path, out)?;
            } else if file_type.is_file() {
                let top_level_doc =
                    dir == root && (name.starts_with("README") || name.starts_with("LICENSE"));
                if name.ends_with(".vex") || top_level_doc {
                    out.push(path.strip_prefix(root)?.to_path_buf());
                }
            }
        }
        Ok(())
    }
    let manifest_rel = manifest
        .strip_prefix(project_dir)
        .unwrap_or(manifest)
        .to_path_buf();
    let mut files = vec![manifest_rel];
    walk(project_dir, project_dir, &mut files)?;
    files.sort();
    files.dedup();
    Ok(files)
}

/// Builds `out_dir/NAME-VERSION.tar.gz` from `project_dir`.
///
/// The archive is reproducible: entries are sorted, live under
/// `NAME-VERSION/`, and carry fixed mtimes, owners and modes, so the same
/// sources always give the same SHA-256.
pub fn package(
    project_dir: &Path,
    manifest: &Path,
    name: &str,
    version: &str,
    out_dir: &Path,
) -> Result<(PathBuf, Vec<PathBuf>)> {
    validate_component("name", name)?;
    validate_component("version", version)?;
    let files = package_files(project_dir, manifest)?;
    let prefix = format!("{name}-{version}");

    let gz = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    tar.mode(tar::HeaderMode::Deterministic);
    for rel in &files {
        let data = fs::read(project_dir.join(rel))
            .with_context(|| format!("cannot read {}", rel.display()))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_entry_type(tar::EntryType::Regular);
        tar.append_data(&mut header, Path::new(&prefix).join(rel), data.as_slice())?;
    }
    let bytes = tar.into_inner()?.finish()?;

    fs::create_dir_all(out_dir).with_context(|| format!("cannot create {}", out_dir.display()))?;
    let dest = out_dir.join(format!("{prefix}.tar.gz"));
    let mut file =
        fs::File::create(&dest).with_context(|| format!("cannot write {}", dest.display()))?;
    file.write_all(&bytes)?;
    Ok((dest, files))
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

    fn sample_project(dir: &Path) -> PathBuf {
        fs::create_dir_all(dir.join("src/util")).unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::create_dir_all(dir.join("target")).unwrap();
        fs::write(
            dir.join("vex.toml"),
            "name = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(dir.join("README.md"), "# demo\n").unwrap();
        fs::write(dir.join("src/main.vex"), "println(\"hi\");\n").unwrap();
        fs::write(dir.join("src/util/math.vex"), "fn two() -> i32 { 2 }\n").unwrap();
        fs::write(dir.join("src/notes.txt"), "not packaged").unwrap();
        fs::write(dir.join(".git/config"), "secret").unwrap();
        fs::write(dir.join("target/junk.vex"), "skip").unwrap();
        dir.join("vex.toml")
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
    fn sha256_matches_known_digest() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn package_selects_sources_and_is_reproducible() {
        let dir = temp_dir("pkg");
        let manifest = sample_project(&dir.join("proj"));
        let files = package_files(&dir.join("proj"), &manifest).unwrap();
        let names: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
        assert_eq!(
            names,
            ["README.md", "src/main.vex", "src/util/math.vex", "vex.toml"]
        );

        let (a, _) = package(
            &dir.join("proj"),
            &manifest,
            "demo",
            "0.1.0",
            &dir.join("a"),
        )
        .unwrap();
        let (b, _) = package(
            &dir.join("proj"),
            &manifest,
            "demo",
            "0.1.0",
            &dir.join("b"),
        )
        .unwrap();
        assert_eq!(a.file_name().unwrap(), "demo-0.1.0.tar.gz");
        assert_eq!(sha256_file(&a).unwrap(), sha256_file(&b).unwrap());

        let mut archive =
            tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&a).unwrap()));
        let mut paths: Vec<String> = archive
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().display().to_string())
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            [
                "demo-0.1.0/README.md",
                "demo-0.1.0/src/main.vex",
                "demo-0.1.0/src/util/math.vex",
                "demo-0.1.0/vex.toml"
            ]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn publish_then_add_round_trip() {
        let dir = temp_dir("roundtrip");
        let manifest = sample_project(&dir.join("proj"));
        let (tarball, _) = package(
            &dir.join("proj"),
            &manifest,
            "demo",
            "0.1.0",
            &dir.join("dist"),
        )
        .unwrap();
        let index_dir = dir.join("index");
        fs::create_dir_all(&index_dir).unwrap();

        let published = publish(
            &index_dir,
            "demo",
            "0.1.0",
            &tarball,
            "https://example.invalid/demo-0.1.0.tar.gz",
            Some("Demo package"),
        )
        .unwrap();
        assert_eq!(published.entry.sha256, sha256_file(&tarball).unwrap());

        // Immutability.
        let again = publish(&index_dir, "demo", "0.1.0", &tarball, "https://x/y", None);
        assert!(again.unwrap_err().to_string().contains("immutable"));

        // Point the entry at the local file so `add` can run offline.
        let mut index = load_index(&index_dir.to_string_lossy()).unwrap();
        assert_eq!(index.packages["demo"].description, "Demo package");
        index
            .packages
            .get_mut("demo")
            .unwrap()
            .versions
            .get_mut("0.1.0")
            .unwrap()
            .url = format!("file://{}", tarball.display());
        let dest = add(&index, "demo", "0.1.0", &dir.join("vendor")).unwrap();
        assert_eq!(sha256_file(&dest).unwrap(), published.entry.sha256);

        // Tampered tarball is rejected and nothing is written.
        fs::write(&tarball, b"tampered").unwrap();
        index
            .packages
            .get_mut("demo")
            .unwrap()
            .versions
            .get_mut("0.1.0")
            .unwrap()
            .size = 8;
        let err = add(&index, "demo", "0.1.0", &dir.join("vendor2")).unwrap_err();
        assert!(err.to_string().contains("SHA-256 mismatch"), "{err}");
        assert!(!dir.join("vendor2/demo-0.1.0.tar.gz").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn publish_requires_https() {
        let dir = temp_dir("https");
        fs::write(dir.join("t.tar.gz"), b"x").unwrap();
        let err = publish(
            &dir,
            "p",
            "1.0.0",
            &dir.join("t.tar.gz"),
            "http://x/t",
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("https://"), "{err}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn resolve_reports_missing_and_yanked() {
        let mut index = Index::default();
        let mut pkg = PackageEntry::default();
        pkg.versions.insert(
            "1.0.0".into(),
            VersionEntry {
                url: "https://x".into(),
                sha256: "00".into(),
                size: 1,
                yanked: true,
            },
        );
        index.packages.insert("p".into(), pkg);
        assert!(
            index
                .resolve("q", "1.0.0")
                .unwrap_err()
                .to_string()
                .contains("not in the index")
        );
        assert!(
            index
                .resolve("p", "2.0.0")
                .unwrap_err()
                .to_string()
                .contains("available: 1.0.0")
        );
        assert!(
            index
                .resolve("p", "1.0.0")
                .unwrap_err()
                .to_string()
                .contains("yanked")
        );
    }

    #[test]
    fn rejects_unknown_schema() {
        let dir = temp_dir("schema");
        fs::write(dir.join(INDEX_FILE), r#"{"schema": 99, "packages": {}}"#).unwrap();
        let err = load_index(&dir.to_string_lossy()).unwrap_err();
        assert!(err.to_string().contains("schema 99"), "{err}");
        fs::remove_dir_all(dir).unwrap();
    }
}
