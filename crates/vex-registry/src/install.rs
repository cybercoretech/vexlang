//! Dependency installation: resolve, lock, download, verify and unpack.
//!
//! `install` works in two phases so a failure never leaves a half-updated
//! project:
//!
//! 1. **Resolve.** Starting from the manifest's dependencies, pin each
//!    `name@version` (from `vex.lock` when it is already locked, else from the
//!    index), fetch its tarball into `vex_modules/.cache/` (verifying size and
//!    SHA-256), read the package's own `vex.toml` straight from the tarball and
//!    queue its dependencies. Versions are exact; two different versions of
//!    one package anywhere in the tree are a conflict.
//! 2. **Apply.** With `--locked`, stop if the result differs from `vex.lock`.
//!    Otherwise unpack changed packages into `vex_modules/NAME/` (staged, then
//!    renamed), remove packages that are no longer needed, and write `vex.lock`.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::{
    Index, MAX_TARBALL_BYTES, VersionEntry, fetch, load_index, sha256_hex, validate_component,
};

pub const LOCK_FILE: &str = "vex.lock";
pub const MODULES_DIR: &str = "vex_modules";
const CACHE_DIR: &str = ".cache";
const MARKER: &str = ".vex-installed";
const LOCK_VERSION: u32 = 1;
const MAX_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lock {
    pub version: u32,
    pub packages: Vec<LockedPackage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    /// `name@version` of each direct dependency.
    #[serde(default)]
    pub dependencies: Vec<String>,
}

pub fn read_lock(root: &Path) -> Result<Option<Lock>> {
    let path = root.join(LOCK_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let text =
        fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    let lock: Lock = serde_json::from_str(&text)
        .with_context(|| format!("{} is not a valid lockfile", path.display()))?;
    if lock.version != LOCK_VERSION {
        bail!(
            "{} has lock version {}, expected {LOCK_VERSION}",
            path.display(),
            lock.version
        );
    }
    Ok(Some(lock))
}

fn write_lock(root: &Path, lock: &Lock) -> Result<()> {
    let mut json = serde_json::to_string_pretty(lock)?;
    json.push('\n');
    let path = root.join(LOCK_FILE);
    let partial = root.join(".vex.lock.part");
    fs::write(&partial, json).with_context(|| format!("cannot write {}", partial.display()))?;
    fs::rename(&partial, &path).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(())
}

#[derive(Debug, Default, Clone, Copy)]
pub struct InstallOptions {
    /// Fail instead of changing `vex.lock`.
    pub locked: bool,
    /// Use only `vex.lock` and the download cache; never touch the network.
    pub offline: bool,
}

#[derive(Debug, Default)]
pub struct InstallReport {
    /// `name@version` that were unpacked in this run.
    pub installed: Vec<String>,
    /// `name@version` already present and unchanged.
    pub unchanged: Vec<String>,
    /// Package directories removed because nothing needs them any more.
    pub removed: Vec<String>,
    pub lock_changed: bool,
}

/// Parses a package's `vex.toml` text into its dependencies.
pub type ManifestDeps<'a> = &'a dyn Fn(&str) -> Result<BTreeMap<String, String>>;

struct Resolved {
    version: String,
    entry: VersionEntry,
    tarball: PathBuf,
    dependencies: Vec<String>,
}

/// Installs `deps` (name -> exact version) and their dependencies into
/// `root/vex_modules`, keeping `root/vex.lock` in sync.
pub fn install(
    root: &Path,
    deps: &BTreeMap<String, String>,
    index_location: &str,
    read_deps: ManifestDeps,
    options: InstallOptions,
) -> Result<InstallReport> {
    let modules = root.join(MODULES_DIR);
    let cache = modules.join(CACHE_DIR);
    let old_lock = read_lock(root)?;
    let locked: BTreeMap<(String, String), LockedPackage> = old_lock
        .iter()
        .flat_map(|lock| lock.packages.iter())
        .map(|p| ((p.name.clone(), p.version.clone()), p.clone()))
        .collect();

    let mut index: Option<Index> = None;
    let mut resolved: BTreeMap<String, Resolved> = BTreeMap::new();
    let mut requested_by: BTreeMap<String, String> = BTreeMap::new();
    let mut queue: VecDeque<(String, String, String)> = deps
        .iter()
        .map(|(n, v)| (n.clone(), v.clone(), "vex.toml".to_string()))
        .collect();

    // Phase 1: resolve and fetch into the cache.
    while let Some((name, version, from)) = queue.pop_front() {
        validate_component("name", &name)?;
        validate_component("version", &version)?;
        if let Some(existing) = resolved.get(&name) {
            if existing.version != version {
                bail!(
                    "version conflict for `{name}`: {} needs {name}@{}, {from} needs {name}@{version}",
                    requested_by[&name],
                    existing.version
                );
            }
            continue;
        }
        let entry = match locked.get(&(name.clone(), version.clone())) {
            Some(p) => VersionEntry {
                url: p.url.clone(),
                sha256: p.sha256.clone(),
                size: p.size,
                yanked: false,
            },
            None => {
                if options.offline {
                    bail!("{name}@{version} is not in vex.lock and --offline was given");
                }
                if index.is_none() {
                    index = Some(load_index(index_location)?);
                }
                index.as_ref().unwrap().resolve(&name, &version)?.clone()
            }
        };
        let tarball = cache.join(format!("{name}-{version}.tar.gz"));
        let bytes = cached_or_fetch(&tarball, &entry, &name, &version, options.offline)?;
        let manifest = manifest_from_tarball(&bytes, &name, &version)?;
        let package_deps = match manifest {
            Some(text) => {
                read_deps(&text).with_context(|| format!("invalid vex.toml in {name}@{version}"))?
            }
            None => BTreeMap::new(),
        };
        let here = format!("{name}@{version}");
        for (dep, dep_version) in &package_deps {
            queue.push_back((dep.clone(), dep_version.clone(), here.clone()));
        }
        requested_by.insert(name.clone(), from);
        resolved.insert(
            name,
            Resolved {
                version,
                entry,
                tarball,
                dependencies: package_deps
                    .iter()
                    .map(|(n, v)| format!("{n}@{v}"))
                    .collect(),
            },
        );
    }

    let new_lock = Lock {
        version: LOCK_VERSION,
        packages: resolved
            .iter()
            .map(|(name, r)| LockedPackage {
                name: name.clone(),
                version: r.version.clone(),
                url: r.entry.url.clone(),
                sha256: r.entry.sha256.to_ascii_lowercase(),
                size: r.entry.size,
                dependencies: r.dependencies.clone(),
            })
            .collect(),
    };
    let lock_changed = old_lock.as_ref() != Some(&new_lock)
        && !(old_lock.is_none() && new_lock.packages.is_empty());
    if options.locked && lock_changed {
        bail!("vex.lock is out of date with vex.toml; run `vexlang install` without --locked");
    }

    // Phase 2: apply.
    let mut report = InstallReport {
        lock_changed,
        ..InstallReport::default()
    };
    for (name, r) in &resolved {
        let dest = modules.join(name);
        let id = format!("{name}@{}", r.version);
        let marker = format!("{id} {}\n", r.entry.sha256.to_ascii_lowercase());
        if fs::read_to_string(dest.join(MARKER)).ok().as_deref() == Some(marker.as_str()) {
            report.unchanged.push(id);
            continue;
        }
        let bytes = fs::read(&r.tarball)?;
        let staging = modules.join(format!(".{name}.staging"));
        let _ = fs::remove_dir_all(&staging);
        unpack(&bytes, name, &r.version, &staging)?;
        fs::write(staging.join(MARKER), &marker)?;
        if dest.exists() {
            fs::remove_dir_all(&dest)
                .with_context(|| format!("cannot replace {}", dest.display()))?;
        }
        fs::rename(&staging, &dest)
            .with_context(|| format!("cannot install {}", dest.display()))?;
        report.installed.push(id);
    }
    if modules.is_dir() {
        let keep: BTreeSet<&str> = resolved.keys().map(String::as_str).collect();
        for entry in fs::read_dir(&modules)? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if file_name.starts_with('.') || keep.contains(file_name.as_str()) {
                continue;
            }
            // Only remove directories this tool installed.
            if entry.path().join(MARKER).is_file() {
                fs::remove_dir_all(entry.path())?;
                report.removed.push(file_name);
            }
        }
        if cache.is_dir() {
            let wanted: BTreeSet<PathBuf> = resolved.values().map(|r| r.tarball.clone()).collect();
            for entry in fs::read_dir(&cache)? {
                let path = entry?.path();
                if !wanted.contains(&path) {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }
    if lock_changed {
        write_lock(root, &new_lock)?;
    }
    Ok(report)
}

fn verify(bytes: &[u8], entry: &VersionEntry, name: &str, version: &str) -> Result<()> {
    if bytes.len() as u64 != entry.size {
        bail!(
            "{name}@{version}: got {} bytes, expected {}",
            bytes.len(),
            entry.size
        );
    }
    let actual = sha256_hex(bytes);
    if !actual.eq_ignore_ascii_case(&entry.sha256) {
        bail!(
            "{name}@{version}: SHA-256 mismatch (expected {}, got {actual})",
            entry.sha256
        );
    }
    Ok(())
}

fn cached_or_fetch(
    tarball: &Path,
    entry: &VersionEntry,
    name: &str,
    version: &str,
    offline: bool,
) -> Result<Vec<u8>> {
    if let Ok(bytes) = fs::read(tarball) {
        if verify(&bytes, entry, name, version).is_ok() {
            return Ok(bytes);
        }
    }
    if offline {
        bail!("{name}@{version} is not in the download cache and --offline was given");
    }
    let bytes = fetch(&entry.url, MAX_TARBALL_BYTES.min(entry.size))?;
    verify(&bytes, entry, name, version)?;
    let dir = tarball.parent().unwrap();
    fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let partial = tarball.with_extension("part");
    fs::write(&partial, &bytes)?;
    fs::rename(&partial, tarball)?;
    Ok(bytes)
}

/// Each archive entry as (path relative to `NAME-VERSION/`, is_dir, data).
/// Rejects anything outside that prefix, `..`, absolute paths, links and
/// special files, and enforces size and entry-count limits.
fn entries(bytes: &[u8], name: &str, version: &str) -> Result<Vec<(PathBuf, bool, Vec<u8>)>> {
    let prefix = format!("{name}-{version}");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    let mut out = Vec::new();
    let mut total = 0u64;
    for entry in archive.entries().context("not a valid .tar.gz")? {
        let mut entry = entry.context("corrupt tarball")?;
        let path = entry.path()?.into_owned();
        let mut components = path.components();
        if components.next() != Some(Component::Normal(prefix.as_ref())) {
            bail!(
                "{name}@{version}: entry `{}` is outside `{prefix}/`",
                path.display()
            );
        }
        let rel: PathBuf = components.collect();
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            bail!("{name}@{version}: unsafe path `{}`", path.display());
        }
        let kind = entry.header().entry_type();
        let is_dir = kind.is_dir();
        if !is_dir && !kind.is_file() {
            bail!(
                "{name}@{version}: `{}` is not a regular file or directory",
                path.display()
            );
        }
        let mut data = Vec::new();
        if !is_dir {
            total += entry.header().size()?;
            if total > MAX_UNPACKED_BYTES {
                bail!("{name}@{version}: unpacks to more than {MAX_UNPACKED_BYTES} bytes");
            }
            entry.read_to_end(&mut data)?;
        }
        out.push((rel, is_dir, data));
        if out.len() > MAX_ENTRIES {
            bail!("{name}@{version}: more than {MAX_ENTRIES} entries");
        }
    }
    Ok(out)
}

fn manifest_from_tarball(bytes: &[u8], name: &str, version: &str) -> Result<Option<String>> {
    for (rel, is_dir, data) in entries(bytes, name, version)? {
        if !is_dir && rel == Path::new("vex.toml") {
            return Ok(Some(
                String::from_utf8(data).context("vex.toml is not UTF-8")?,
            ));
        }
    }
    Ok(None)
}

fn unpack(bytes: &[u8], name: &str, version: &str, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for (rel, is_dir, data) in entries(bytes, name, version)? {
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = dest.join(&rel);
        if is_dir {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&target, data)
                .with_context(|| format!("cannot write {}", target.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{package, sha256_file};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vex-install-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Minimal `[dependencies]` reader for tests (the CLI uses vexlang's own parser).
    fn deps_of(text: &str) -> Result<BTreeMap<String, String>> {
        let mut deps = BTreeMap::new();
        let mut in_deps = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                in_deps = line == "[dependencies]";
            } else if in_deps {
                if let Some((k, v)) = line.split_once('=') {
                    deps.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
                }
            }
        }
        Ok(deps)
    }

    /// Builds a package and adds it to `index` with a file:// URL.
    fn publish_local(
        dir: &Path,
        index: &mut Index,
        name: &str,
        version: &str,
        deps: &[(&str, &str)],
    ) {
        let proj = dir.join(format!("src-{name}-{version}"));
        fs::create_dir_all(proj.join("src")).unwrap();
        let mut manifest = format!("name = \"{name}\"\nversion = \"{version}\"\n");
        if !deps.is_empty() {
            manifest.push_str("[dependencies]\n");
            for (d, v) in deps {
                manifest.push_str(&format!("{d} = \"{v}\"\n"));
            }
        }
        fs::write(proj.join("vex.toml"), manifest).unwrap();
        fs::write(
            proj.join("src/lib.vex"),
            format!("fn {name}_v() -> i32 {{ 1 }}\n"),
        )
        .unwrap();
        let (tarball, _) = package(
            &proj,
            &proj.join("vex.toml"),
            name,
            version,
            &dir.join("dist"),
        )
        .unwrap();
        index
            .packages
            .entry(name.to_string())
            .or_default()
            .versions
            .insert(
                version.to_string(),
                VersionEntry {
                    url: format!("file://{}", tarball.display()),
                    sha256: sha256_file(&tarball).unwrap(),
                    size: fs::metadata(&tarball).unwrap().len(),
                    yanked: false,
                },
            );
    }

    fn write_index(dir: &Path, index: &Index) -> String {
        let path = dir.join("index.json");
        fs::write(&path, serde_json::to_string(index).unwrap()).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn deps(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn installs_transitively_locks_and_is_idempotent() {
        let dir = temp_dir("transitive");
        let mut index = Index::default();
        publish_local(&dir, &mut index, "base", "1.0.0", &[]);
        publish_local(&dir, &mut index, "mid", "0.2.0", &[("base", "1.0.0")]);
        let location = write_index(&dir, &index);
        let root = dir.join("app");
        fs::create_dir_all(&root).unwrap();

        let report = install(
            &root,
            &deps(&[("mid", "0.2.0")]),
            &location,
            &deps_of,
            InstallOptions::default(),
        )
        .unwrap();
        assert_eq!(report.installed, ["base@1.0.0", "mid@0.2.0"]);
        assert!(report.lock_changed);
        assert!(root.join("vex_modules/base/src/lib.vex").is_file());
        assert!(root.join("vex_modules/mid/vex.toml").is_file());
        let lock = read_lock(&root).unwrap().unwrap();
        assert_eq!(lock.packages.len(), 2);
        assert_eq!(lock.packages[1].dependencies, ["base@1.0.0"]);

        let again = install(
            &root,
            &deps(&[("mid", "0.2.0")]),
            &location,
            &deps_of,
            InstallOptions::default(),
        )
        .unwrap();
        assert!(again.installed.is_empty() && !again.lock_changed);
        assert_eq!(again.unchanged.len(), 2);

        // --offline works from lock + cache even with the index gone.
        fs::remove_file(&location).unwrap();
        fs::remove_dir_all(root.join("vex_modules/base")).unwrap();
        let offline = install(
            &root,
            &deps(&[("mid", "0.2.0")]),
            &location,
            &deps_of,
            InstallOptions {
                offline: true,
                locked: true,
            },
        )
        .unwrap();
        assert_eq!(offline.installed, ["base@1.0.0"]);

        // Dropping the dependency prunes both packages and empties the lock.
        let pruned = install(
            &root,
            &BTreeMap::new(),
            &location,
            &deps_of,
            InstallOptions::default(),
        )
        .unwrap();
        assert_eq!(pruned.removed.len(), 2);
        assert!(read_lock(&root).unwrap().unwrap().packages.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reports_version_conflicts_without_touching_the_project() {
        let dir = temp_dir("conflict");
        let mut index = Index::default();
        publish_local(&dir, &mut index, "base", "1.0.0", &[]);
        publish_local(&dir, &mut index, "base", "2.0.0", &[]);
        publish_local(&dir, &mut index, "mid", "0.1.0", &[("base", "1.0.0")]);
        let location = write_index(&dir, &index);
        let root = dir.join("app");
        fs::create_dir_all(&root).unwrap();
        let err = install(
            &root,
            &deps(&[("base", "2.0.0"), ("mid", "0.1.0")]),
            &location,
            &deps_of,
            InstallOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("version conflict for `base`"), "{err}");
        assert!(!root.join("vex.lock").exists());
        assert!(!root.join("vex_modules/base").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn locked_refuses_changes() {
        let dir = temp_dir("locked");
        let mut index = Index::default();
        publish_local(&dir, &mut index, "base", "1.0.0", &[]);
        let location = write_index(&dir, &index);
        let root = dir.join("app");
        fs::create_dir_all(&root).unwrap();
        let err = install(
            &root,
            &deps(&[("base", "1.0.0")]),
            &location,
            &deps_of,
            InstallOptions {
                locked: true,
                offline: false,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("out of date"), "{err}");
        fs::remove_dir_all(dir).unwrap();
    }

    fn tarball_with(entries: &[(&str, tar::EntryType, &[u8])]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(gz);
        for (path, kind, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(*kind);
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            // Write the raw name so `..` is not normalised away.
            let name = header.as_old_mut();
            name.name[..path.len()].copy_from_slice(path.as_bytes());
            header.set_cksum();
            builder.append(&header, *data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn unpack_rejects_unsafe_archives() {
        let dir = temp_dir("unsafe");
        for (path, kind) in [
            ("p-1/../../escape", tar::EntryType::Regular),
            ("other-1/file", tar::EntryType::Regular),
            ("p-1/link", tar::EntryType::Symlink),
            ("/abs", tar::EntryType::Regular),
        ] {
            let bytes = tarball_with(&[(path, kind, b"x")]);
            assert!(
                unpack(&bytes, "p", "1", &dir.join("out")).is_err(),
                "{path} accepted"
            );
        }
        assert!(!dir.join("escape").exists());
        let ok = tarball_with(&[(
            "p-1/src/lib.vex",
            tar::EntryType::Regular,
            b"fn a() -> i32 { 1 }",
        )]);
        unpack(&ok, "p", "1", &dir.join("good")).unwrap();
        assert!(dir.join("good/src/lib.vex").is_file());
        fs::remove_dir_all(dir).unwrap();
    }
}
