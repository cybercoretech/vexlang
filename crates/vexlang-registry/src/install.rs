//! Dependency installation: resolve, lock, download, verify and unpack.
//!
//! `install` works in two phases so a failure never leaves a half-updated
//! project:
//!
//! 1. **Resolve.** Dependencies are version requirements (see [`crate::req`]):
//!    `"0.1.0"` means `^0.1.0`. Starting from the manifest, each package gets
//!    the version `vex.lock` already pins if it still satisfies every
//!    requirement seen so far (unless it is being updated), otherwise the
//!    newest non-yanked matching version in the index. Its tarball is fetched
//!    into `vex_modules/.cache/` (verifying size and SHA-256), and the
//!    package's own `vex.toml` is read straight from the tarball to queue its
//!    dependencies. Each package has one version in the whole tree: if a
//!    later requirement rules out an earlier choice, the package is
//!    re-picked to satisfy all of them and resolution starts over. If no
//!    version satisfies all of them, that's a conflict that lists who needs
//!    what.
//! 2. **Apply.** With `--locked`, stop if the result differs from `vex.lock`.
//!    Otherwise unpack changed packages into `vex_modules/NAME/` (staged, then
//!    renamed), remove packages that are no longer needed, and write `vex.lock`.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::req::{Version, VersionReq};
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

#[derive(Debug, Default, Clone)]
pub struct InstallOptions {
    /// Fail instead of changing `vex.lock`.
    pub locked: bool,
    /// Use only `vex.lock` and the download cache; never touch the network.
    pub offline: bool,
    /// Ignore the versions `vex.lock` pins and take the newest matching ones.
    pub update: Update,
}

/// Which locked versions `install` may move away from.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Update {
    /// Keep every locked version that still satisfies its requirements.
    #[default]
    Nothing,
    /// Re-resolve everything (`vexlang update`).
    All,
    /// Re-resolve these packages only (`vexlang update NAME...`).
    Only(BTreeSet<String>),
}

impl Update {
    fn includes(&self, name: &str) -> bool {
        match self {
            Update::Nothing => false,
            Update::All => true,
            Update::Only(names) => names.contains(name),
        }
    }
}

#[derive(Debug, Default)]
pub struct InstallReport {
    /// `name@version` that were unpacked in this run.
    pub installed: Vec<String>,
    /// `name@version` already present and unchanged.
    pub unchanged: Vec<String>,
    /// Package directories removed because nothing needs them any more.
    pub removed: Vec<String>,
    /// `(name, old version, new version)` for packages whose locked version
    /// changed.
    pub changed: Vec<(String, String, String)>,
    pub lock_changed: bool,
}

/// Parses a package's `vex.toml` text into its dependencies
/// (name -> version requirement).
pub type ManifestDeps<'a> = &'a dyn Fn(&str) -> Result<BTreeMap<String, String>>;

struct Resolved {
    version: String,
    entry: VersionEntry,
    tarball: PathBuf,
    dependencies: Vec<String>,
}

/// A fetched package: its index entry, cached tarball and dependencies.
struct Fetched {
    entry: VersionEntry,
    tarball: PathBuf,
    deps: BTreeMap<String, String>,
}

/// How many times resolution may start over before giving up.
const MAX_ROUNDS: usize = 64;

struct Resolver<'a> {
    index_location: &'a str,
    index: Option<Index>,
    locked: BTreeMap<String, LockedPackage>,
    options: &'a InstallOptions,
    cache: PathBuf,
    read_deps: ManifestDeps<'a>,
    fetched: BTreeMap<(String, String), Fetched>,
}

impl Resolver<'_> {
    fn index(&mut self) -> Result<&Index> {
        if self.index.is_none() {
            self.index = Some(load_index(self.index_location)?);
        }
        Ok(self.index.as_ref().unwrap())
    }

    /// Picks a version of `name` that satisfies `reqs`: `preferred` (a
    /// version chosen in an earlier round), then the locked one, then the
    /// newest non-yanked match in the index.
    fn choose(
        &mut self,
        name: &str,
        reqs: &[(VersionReq, String)],
        preferred: Option<&str>,
    ) -> Result<String> {
        let fits = |version: &str| {
            Version::parse(version).is_ok_and(|v| reqs.iter().all(|(r, _)| r.matches(&v)))
        };
        if let Some(version) = preferred.filter(|v| fits(v)) {
            return Ok(version.to_string());
        }
        if !self.options.update.includes(name) {
            if let Some(locked) = self.locked.get(name).filter(|p| fits(&p.version)) {
                return Ok(locked.version.clone());
            }
        }
        let wanted = describe(reqs);
        if self.options.offline {
            bail!(
                "vex.lock has no version of `{name}` that satisfies {wanted}, and --offline was given"
            );
        }
        let package = self
            .index()?
            .packages
            .get(name)
            .with_context(|| format!("package `{name}` is not in the index"))?;
        let best = package
            .versions
            .iter()
            .filter(|(version, entry)| !entry.yanked && fits(version))
            .max_by(|(a, _), (b, _)| crate::compare_versions(a, b));
        match best {
            Some((version, _)) => Ok(version.clone()),
            None => {
                let available: Vec<&str> = package
                    .versions
                    .iter()
                    .filter(|(_, e)| !e.yanked)
                    .map(|(v, _)| v.as_str())
                    .collect();
                bail!(
                    "no version of `{name}` satisfies {wanted} (available: {})",
                    if available.is_empty() {
                        "none".to_string()
                    } else {
                        available.join(", ")
                    }
                )
            }
        }
    }

    /// Fetches `name@version` (cached) and reads its dependencies.
    fn fetch(&mut self, name: &str, version: &str) -> Result<&Fetched> {
        let key = (name.to_string(), version.to_string());
        if !self.fetched.contains_key(&key) {
            let entry = match self.locked.get(name).filter(|p| p.version == version) {
                Some(p) => VersionEntry {
                    url: p.url.clone(),
                    sha256: p.sha256.clone(),
                    size: p.size,
                    yanked: false,
                },
                None => self.index()?.resolve(name, version)?.clone(),
            };
            let tarball = self.cache.join(format!("{name}-{version}.tar.gz"));
            let bytes = cached_or_fetch(&tarball, &entry, name, version, self.options.offline)?;
            let deps = match manifest_from_tarball(&bytes, name, version)? {
                Some(text) => (self.read_deps)(&text)
                    .with_context(|| format!("invalid vex.toml in {name}@{version}"))?,
                None => BTreeMap::new(),
            };
            self.fetched.insert(
                key.clone(),
                Fetched {
                    entry,
                    tarball,
                    deps,
                },
            );
        }
        Ok(&self.fetched[&key])
    }

    /// Resolves the whole tree: name -> version.
    fn resolve(&mut self, deps: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>> {
        let mut picks: BTreeMap<String, String> = BTreeMap::new();
        for _ in 0..MAX_ROUNDS {
            let mut reqs: BTreeMap<String, Vec<(VersionReq, String)>> = BTreeMap::new();
            let mut chosen: BTreeMap<String, String> = BTreeMap::new();
            let mut queue: VecDeque<(String, String, String)> = deps
                .iter()
                .map(|(n, v)| (n.clone(), v.clone(), "vex.toml".to_string()))
                .collect();
            while let Some((name, text, from)) = queue.pop_front() {
                validate_component("name", &name)?;
                let req = VersionReq::parse(&text)
                    .with_context(|| format!("{from}: dependency `{name}`"))?;
                let these = reqs.entry(name.clone()).or_default();
                these.push((req, from));
                if chosen.contains_key(&name) {
                    continue;
                }
                let these = these.clone();
                let version = self.choose(&name, &these, picks.get(&name).map(String::as_str))?;
                let here = format!("{name}@{version}");
                for (dep, dep_req) in &self.fetch(&name, &version)?.deps {
                    queue.push_back((dep.clone(), dep_req.clone(), here.clone()));
                }
                chosen.insert(name, version);
            }
            let mut settled = true;
            for (name, version) in &chosen {
                let all = &reqs[name];
                let parsed = Version::parse(version)?;
                if all.iter().all(|(r, _)| r.matches(&parsed)) {
                    continue;
                }
                let better = self.choose(name, all, None).map_err(|error| {
                    anyhow::anyhow!(
                        "version conflict for `{name}`: {}\n{error}",
                        all.iter()
                            .map(|(r, from)| format!("{from} needs {name} {r}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })?;
                picks.insert(name.clone(), better);
                settled = false;
            }
            if settled {
                return Ok(chosen);
            }
        }
        bail!("dependency versions did not settle after {MAX_ROUNDS} rounds")
    }
}

fn describe(reqs: &[(VersionReq, String)]) -> String {
    reqs.iter()
        .map(|(r, from)| format!("`{r}` ({from})"))
        .collect::<Vec<_>>()
        .join(" and ")
}

/// Installs `deps` (name -> version requirement) and their dependencies into
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

    // Phase 1: resolve and fetch into the cache.
    let mut resolver = Resolver {
        index_location,
        index: None,
        locked: old_lock
            .iter()
            .flat_map(|lock| lock.packages.iter())
            .map(|p| (p.name.clone(), p.clone()))
            .collect(),
        options: &options,
        cache: cache.clone(),
        read_deps,
        fetched: BTreeMap::new(),
    };
    let chosen = resolver.resolve(deps)?;
    let mut resolved: BTreeMap<String, Resolved> = BTreeMap::new();
    for (name, version) in &chosen {
        let fetched = resolver.fetch(name, version)?;
        resolved.insert(
            name.clone(),
            Resolved {
                version: version.clone(),
                entry: fetched.entry.clone(),
                tarball: fetched.tarball.clone(),
                dependencies: fetched
                    .deps
                    .keys()
                    .map(|dep| format!("{dep}@{}", chosen[dep]))
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
    for old in old_lock.iter().flat_map(|lock| lock.packages.iter()) {
        if let Some(new) = resolved.get(&old.name).filter(|r| r.version != old.version) {
            report
                .changed
                .push((old.name.clone(), old.version.clone(), new.version.clone()));
        }
    }
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
                ..InstallOptions::default()
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
        assert!(
            err.contains("vex.toml needs base 2.0.0") && err.contains("mid@0.1.0 needs base 1.0.0"),
            "{err}"
        );
        assert!(!root.join("vex.lock").exists());
        assert!(!root.join("vex_modules/base").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    fn install_with(
        root: &Path,
        pairs: &[(&str, &str)],
        location: &str,
        options: InstallOptions,
    ) -> Result<InstallReport> {
        install(root, &deps(pairs), location, &deps_of, options)
    }

    fn locked_versions(root: &Path) -> Vec<String> {
        read_lock(root)
            .unwrap()
            .unwrap()
            .packages
            .iter()
            .map(|p| format!("{}@{}", p.name, p.version))
            .collect()
    }

    #[test]
    fn ranges_pick_the_newest_match_and_keep_locked_versions() {
        let dir = temp_dir("ranges");
        let mut index = Index::default();
        for v in ["1.0.0", "1.1.0", "2.0.0", "1.2.0-rc.1"] {
            publish_local(&dir, &mut index, "base", v, &[]);
        }
        let location = write_index(&dir, &index);
        let root = dir.join("app");
        fs::create_dir_all(&root).unwrap();
        let default = InstallOptions::default;

        install_with(&root, &[("base", "^1.0")], &location, default()).unwrap();
        assert_eq!(locked_versions(&root), ["base@1.1.0"]);

        // A newer match appears: install keeps the lock, update moves it.
        publish_local(&dir, &mut index, "base", "1.3.0", &[]);
        let location = write_index(&dir, &index);
        let report = install_with(&root, &[("base", "^1.0")], &location, default()).unwrap();
        assert!(!report.lock_changed && report.changed.is_empty());
        let report = install_with(
            &root,
            &[("base", "^1.0")],
            &location,
            InstallOptions {
                update: Update::All,
                ..default()
            },
        )
        .unwrap();
        assert_eq!(
            report.changed,
            [("base".to_string(), "1.1.0".to_string(), "1.3.0".to_string())]
        );
        assert_eq!(locked_versions(&root), ["base@1.3.0"]);

        // Tightening the requirement re-resolves without `update`.
        install_with(&root, &[("base", "~1.1")], &location, default()).unwrap();
        assert_eq!(locked_versions(&root), ["base@1.1.0"]);
        install_with(&root, &[("base", "=1.0.0")], &location, default()).unwrap();
        assert_eq!(locked_versions(&root), ["base@1.0.0"]);
        install_with(&root, &[("base", "*")], &location, default()).unwrap();
        assert_eq!(
            locked_versions(&root),
            ["base@1.0.0"],
            "lock still fits `*`"
        );

        let err = install_with(&root, &[("base", "^3")], &location, default())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("no version of `base` satisfies `^3`")
                && err.contains("available: 1.0.0, 1.1.0, 1.2.0-rc.1, 1.3.0, 2.0.0"),
            "{err}"
        );
        let err = install_with(&root, &[("base", "1.x.y")], &location, default()).unwrap_err();
        assert!(
            format!("{err:#}").contains("invalid version requirement"),
            "{err:#}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn shared_dependencies_are_re_picked_to_satisfy_everyone() {
        let dir = temp_dir("repick");
        let mut index = Index::default();
        publish_local(&dir, &mut index, "c", "1.0.0", &[]);
        publish_local(&dir, &mut index, "c", "1.1.0", &[]);
        publish_local(&dir, &mut index, "a", "1.0.0", &[("c", "^1")]);
        publish_local(&dir, &mut index, "b", "1.0.0", &[("c", "=1.0.0")]);
        let location = write_index(&dir, &index);
        let root = dir.join("app");
        fs::create_dir_all(&root).unwrap();
        // `a` is resolved first and would pick c@1.1.0; `b` then needs
        // exactly 1.0.0, so c is re-picked.
        install_with(
            &root,
            &[("a", "1"), ("b", "1")],
            &location,
            InstallOptions::default(),
        )
        .unwrap();
        assert_eq!(locked_versions(&root), ["a@1.0.0", "b@1.0.0", "c@1.0.0"]);
        let lock = read_lock(&root).unwrap().unwrap();
        assert_eq!(lock.packages[0].dependencies, ["c@1.0.0"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn update_one_package_keeps_the_others() {
        let dir = temp_dir("update-one");
        let mut index = Index::default();
        publish_local(&dir, &mut index, "x", "1.0.0", &[]);
        publish_local(&dir, &mut index, "y", "1.0.0", &[]);
        let location = write_index(&dir, &index);
        let root = dir.join("app");
        fs::create_dir_all(&root).unwrap();
        let pairs = [("x", "1"), ("y", "1")];
        install_with(&root, &pairs, &location, InstallOptions::default()).unwrap();
        publish_local(&dir, &mut index, "x", "1.1.0", &[]);
        publish_local(&dir, &mut index, "y", "1.1.0", &[]);
        let location = write_index(&dir, &index);
        install_with(
            &root,
            &pairs,
            &location,
            InstallOptions {
                update: Update::Only(["y".to_string()].into()),
                ..InstallOptions::default()
            },
        )
        .unwrap();
        assert_eq!(locked_versions(&root), ["x@1.0.0", "y@1.1.0"]);
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
                ..InstallOptions::default()
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
