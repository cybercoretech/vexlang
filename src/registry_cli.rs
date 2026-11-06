//! Package commands on top of `vexlang_registry`:
//! `new | package | publish | yank | add | remove | install | fetch | search | info`.
//!
//! Packages live in a static Git index (no server, no login). See the
//! `vexlang_registry` crate docs for the index and lockfile formats.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use vexlang_registry::install::{InstallOptions, InstallReport};

pub const USAGE: &str = "packages:\n\
       vexlang new NAME [--lib]                         create a project (or library) skeleton\n\
       vexlang add NAME[@VERSION] [--index I]           add a dependency to vex.toml and install it\n\
       vexlang remove NAME                              drop a dependency and uninstall it\n\
       vexlang install [--locked] [--offline]           install vex.toml dependencies into vex_modules/\n\
       vexlang search [TERM] | vexlang info NAME        browse the index\n\
       vexlang fetch NAME@VERSION [--out DIR]           download and verify one tarball\n\
       vexlang package [vex.toml] [--out DIR]           build dist/NAME-VERSION.tar.gz\n\
       vexlang publish [vex.toml] --index DIR --url URL [--file TARBALL] [--description TEXT]\n\
       vexlang yank NAME@VERSION --index DIR            withdraw a version (one-way)\n\
       (--manifest PATH selects vex.toml for add/remove/install; the index defaults to\n\
        $VEX_INDEX, then the cybercoretech/vex-index repository)";

/// Runs a package subcommand. Returns `None` if `command` is not one, so the
/// caller can fall through to the language commands.
pub fn run(command: &str, rest: &[String]) -> Option<i32> {
    let result = match command {
        "new" => new(rest),
        "package" => package(rest),
        "publish" => publish(rest),
        "yank" => yank(rest),
        "add" => add(rest),
        "remove" => remove(rest),
        "install" => install(rest),
        "fetch" => fetch(rest),
        "search" => search(rest),
        "info" => info(rest),
        _ => return None,
    };
    Some(match result {
        Ok(()) => 0,
        Err(CliError::Usage(message)) => {
            eprintln!("error: {message}\n\n{USAGE}");
            2
        }
        Err(CliError::Failed(error)) => {
            eprintln!("error: {error:#}");
            1
        }
    })
}

enum CliError {
    Usage(String),
    Failed(anyhow::Error),
}

impl From<anyhow::Error> for CliError {
    fn from(error: anyhow::Error) -> Self {
        CliError::Failed(error)
    }
}

fn failed(message: impl Into<String>) -> CliError {
    CliError::Failed(anyhow::anyhow!(message.into()))
}

type CliResult = Result<(), CliError>;

/// Parsed `--flag value` pairs, `--switch`es and at most one positional.
struct Flags {
    values: Vec<(String, String)>,
    switches: Vec<String>,
    positional: Option<String>,
}

impl Flags {
    fn parse(rest: &[String], valued: &[&str], switches: &[&str]) -> Result<Self, CliError> {
        let mut flags = Flags {
            values: Vec::new(),
            switches: Vec::new(),
            positional: None,
        };
        let mut iter = rest.iter();
        while let Some(arg) = iter.next() {
            if valued.contains(&arg.as_str()) {
                let value = iter
                    .next()
                    .ok_or_else(|| CliError::Usage(format!("{arg} needs a value")))?;
                flags.values.push((arg.clone(), value.clone()));
            } else if switches.contains(&arg.as_str()) {
                flags.switches.push(arg.clone());
            } else if arg.starts_with("--") || flags.positional.is_some() {
                return Err(CliError::Usage(format!("unexpected argument `{arg}`")));
            } else {
                flags.positional = Some(arg.clone());
            }
        }
        Ok(flags)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn require(&self, name: &str, command: &str) -> Result<&str, CliError> {
        self.get(name)
            .ok_or_else(|| CliError::Usage(format!("{command} requires {name}")))
    }

    fn has(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }

    fn positional(&self, what: &str, command: &str) -> Result<&str, CliError> {
        self.positional
            .as_deref()
            .ok_or_else(|| CliError::Usage(format!("{command} requires {what}")))
    }

    fn index(&self) -> String {
        self.get("--index")
            .map(str::to_string)
            .unwrap_or_else(vexlang_registry::default_index)
    }
}

fn split_spec<'a>(spec: &'a str, command: &str) -> Result<(&'a str, &'a str), CliError> {
    spec.split_once('@')
        .ok_or_else(|| CliError::Usage(format!("{command} expects NAME@VERSION, got `{spec}`")))
}

struct Manifest {
    path: PathBuf,
    dir: PathBuf,
    config: crate::project::ProjectConfig,
}

fn load_manifest(path: &str) -> Result<Manifest, CliError> {
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err(failed(format!(
            "no {} here; run `vexlang new NAME` to start a project",
            path.display()
        )));
    }
    let config = crate::project::load(&path).map_err(failed)?;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf();
    Ok(Manifest { path, dir, config })
}

fn manifest_flag(flags: &Flags) -> Result<Manifest, CliError> {
    load_manifest(flags.get("--manifest").unwrap_or("vex.toml"))
}

fn manifest_positional(flags: &Flags) -> Result<Manifest, CliError> {
    load_manifest(flags.positional.as_deref().unwrap_or("vex.toml"))
}

fn read_deps(text: &str) -> anyhow::Result<BTreeMap<String, String>> {
    crate::project::parse(text)
        .map(|config| config.dependencies)
        .map_err(|e| anyhow::anyhow!(e))
}

fn run_install(manifest: &Manifest, index: &str, options: InstallOptions) -> CliResult {
    let report = vexlang_registry::install::install(
        &manifest.dir,
        &manifest.config.dependencies,
        index,
        &read_deps,
        options,
    )?;
    print_report(&report);
    Ok(())
}

fn print_report(report: &InstallReport) {
    for id in &report.installed {
        println!("  installed {id}");
    }
    for name in &report.removed {
        println!("  removed   {name}");
    }
    let total = report.installed.len() + report.unchanged.len();
    println!(
        "{total} package{} in vex_modules/{}",
        if total == 1 { "" } else { "s" },
        if report.lock_changed {
            " (vex.lock updated)"
        } else {
            ""
        }
    );
}

const GITIGNORE: &str = "vex_modules/\ndist/\n";

fn new(rest: &[String]) -> CliResult {
    let flags = Flags::parse(rest, &[], &["--lib"])?;
    let name = flags.positional("NAME", "new")?;
    vexlang_registry::validate_component("name", name)?;
    let dir = PathBuf::from(name);
    if dir.exists() {
        return Err(failed(format!("`{name}` already exists")));
    }
    let lib = flags.has("--lib");
    fs::create_dir_all(dir.join("src")).map_err(anyhow::Error::from)?;
    let (entry, code) = if lib {
        (
            "src/lib.vex",
            format!(
                "// Public API of `{name}`. Other projects load it with `use {name};`.\n\
                 fn double(x: i32) -> i32 {{ x * 2 }}\n\nexport double;\n"
            ),
        )
    } else {
        ("src/main.vex", "println(\"hello from Vex\");\n".to_string())
    };
    let files = [
        (
            "vex.toml",
            format!(
                "[project]\nname = \"{name}\"\nversion = \"0.1.0\"\ndescription = \"\"\n\n[dependencies]\n"
            ),
        ),
        (entry, code),
        (
            "README.md",
            format!(
                "# {name}\n\n{}\n",
                if lib {
                    format!("A Vex library. Use it with `vexlang add {name}` and `use {name};`.")
                } else {
                    "A Vex program. Run it with `vexlang run src/main.vex`.".to_string()
                }
            ),
        ),
        (".gitignore", GITIGNORE.to_string()),
    ];
    for (file, body) in &files {
        fs::write(dir.join(file), body).map_err(anyhow::Error::from)?;
    }
    println!(
        "created {} `{name}` in {}/",
        if lib { "library" } else { "program" },
        dir.display()
    );
    Ok(())
}

fn package(rest: &[String]) -> CliResult {
    let flags = Flags::parse(rest, &["--out"], &[])?;
    let m = manifest_positional(&flags)?;
    let out = flags
        .get("--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| m.dir.join("dist"));
    let (tarball, files) =
        vexlang_registry::package(&m.dir, &m.path, &m.config.name, &m.config.version, &out)?;
    for file in &files {
        println!("  {}", file.display());
    }
    println!(
        "packaged {}@{} -> {} (sha256 {})",
        m.config.name,
        m.config.version,
        tarball.display(),
        vexlang_registry::sha256_file(&tarball)?
    );
    Ok(())
}

fn publish(rest: &[String]) -> CliResult {
    let flags = Flags::parse(rest, &["--index", "--url", "--file", "--description"], &[])?;
    let m = manifest_positional(&flags)?;
    let index_dir = PathBuf::from(flags.require("--index", "publish")?);
    let url = flags.require("--url", "publish")?;
    let tarball = flags.get("--file").map(PathBuf::from).unwrap_or_else(|| {
        m.dir
            .join("dist")
            .join(format!("{}-{}.tar.gz", m.config.name, m.config.version))
    });
    let description =
        flags
            .get("--description")
            .or(m.config.description.as_deref().filter(|d| !d.is_empty()));

    let published = vexlang_registry::publish(
        &index_dir,
        &m.config.name,
        &m.config.version,
        &tarball,
        url,
        description,
    )?;
    println!(
        "recorded {}@{} in {} (sha256 {}, {} bytes)",
        m.config.name,
        m.config.version,
        published.index_file.display(),
        published.entry.sha256,
        published.entry.size
    );
    println!("next:");
    println!("  1. upload {} so it is served at {url}", tarball.display());
    println!(
        "  2. commit index.json in {} and open a pull request",
        index_dir.display()
    );
    Ok(())
}

fn yank(rest: &[String]) -> CliResult {
    let flags = Flags::parse(rest, &["--index"], &[])?;
    let (name, version) = split_spec(flags.positional("NAME@VERSION", "yank")?, "yank")?;
    let index_dir = PathBuf::from(flags.require("--index", "yank")?);
    let file = vexlang_registry::yank(&index_dir, name, version)?;
    println!("yanked {name}@{version} in {}", file.display());
    println!("next: commit index.json and open a pull request");
    Ok(())
}

fn add(rest: &[String]) -> CliResult {
    let flags = Flags::parse(rest, &["--manifest", "--index"], &[])?;
    let spec = flags.positional("NAME[@VERSION]", "add")?;
    let mut m = manifest_flag(&flags)?;
    let index_location = flags.index();
    let (name, version) = match spec.split_once('@') {
        Some((name, version)) => (name.to_string(), version.to_string()),
        None => {
            let index = vexlang_registry::load_index(&index_location)?;
            let (version, _) = index.latest(spec)?;
            (spec.to_string(), version.to_string())
        }
    };
    vexlang_registry::validate_component("name", &name)?;
    vexlang_registry::validate_component("version", &version)?;
    if name == m.config.name {
        return Err(failed(format!(
            "a project cannot depend on itself (`{name}`)"
        )));
    }

    let original = fs::read_to_string(&m.path).map_err(anyhow::Error::from)?;
    let updated = crate::project::set_dependency(&original, &name, &version);
    m.config = crate::project::parse(&updated).map_err(failed)?;
    fs::write(&m.path, &updated).map_err(anyhow::Error::from)?;
    if let Err(error) = run_install(&m, &index_location, InstallOptions::default()) {
        // Leave vex.toml as it was if the new dependency cannot be installed.
        let _ = fs::write(&m.path, &original);
        return Err(error);
    }
    println!("added {name} = \"{version}\" to {}", m.path.display());
    Ok(())
}

fn remove(rest: &[String]) -> CliResult {
    let flags = Flags::parse(rest, &["--manifest", "--index"], &[])?;
    let name = flags.positional("NAME", "remove")?;
    let mut m = manifest_flag(&flags)?;
    let original = fs::read_to_string(&m.path).map_err(anyhow::Error::from)?;
    let updated = crate::project::remove_dependency(&original, name).ok_or_else(|| {
        failed(format!(
            "`{name}` is not a dependency in {}",
            m.path.display()
        ))
    })?;
    m.config = crate::project::parse(&updated).map_err(failed)?;
    fs::write(&m.path, &updated).map_err(anyhow::Error::from)?;
    run_install(&m, &flags.index(), InstallOptions::default())?;
    println!("removed {name} from {}", m.path.display());
    Ok(())
}

fn install(rest: &[String]) -> CliResult {
    let flags = Flags::parse(rest, &["--manifest", "--index"], &["--locked", "--offline"])?;
    if let Some(extra) = &flags.positional {
        return Err(CliError::Usage(format!("unexpected argument `{extra}`")));
    }
    let m = manifest_flag(&flags)?;
    run_install(
        &m,
        &flags.index(),
        InstallOptions {
            locked: flags.has("--locked"),
            offline: flags.has("--offline"),
        },
    )
}

fn fetch(rest: &[String]) -> CliResult {
    let flags = Flags::parse(rest, &["--out", "--index"], &[])?;
    let (name, version) = split_spec(flags.positional("NAME@VERSION", "fetch")?, "fetch")?;
    let out = PathBuf::from(flags.get("--out").unwrap_or("."));
    let index = vexlang_registry::load_index(&flags.index())?;
    let dest = vexlang_registry::add(&index, name, version, &out)?;
    println!(
        "fetched {name}@{version} -> {} (sha256 verified)",
        dest.display()
    );
    Ok(())
}

fn search(rest: &[String]) -> CliResult {
    let flags = Flags::parse(rest, &["--index"], &[])?;
    let index = vexlang_registry::load_index(&flags.index())?;
    let term = flags.positional.as_deref().unwrap_or("");
    let hits = index.search(term);
    if hits.is_empty() {
        println!("no packages match `{term}`");
    }
    for (name, entry) in hits {
        let latest = index
            .latest(name)
            .map(|(v, _)| v.to_string())
            .unwrap_or_else(|_| "yanked".into());
        println!("{name} {latest}  {}", entry.description);
    }
    Ok(())
}

fn info(rest: &[String]) -> CliResult {
    let flags = Flags::parse(rest, &["--index"], &[])?;
    let name = flags.positional("NAME", "info")?;
    let index = vexlang_registry::load_index(&flags.index())?;
    let entry = index
        .packages
        .get(name)
        .ok_or_else(|| failed(format!("package `{name}` is not in the index")))?;
    println!("{name}");
    if !entry.description.is_empty() {
        println!("  {}", entry.description);
    }
    let mut versions: Vec<_> = entry.versions.iter().collect();
    versions.sort_by(|a, b| vexlang_registry::compare_versions(b.0, a.0));
    for (version, v) in versions {
        println!(
            "  {version}{}  {} bytes  sha256 {}",
            if v.yanked { " (yanked)" } else { "" },
            v.size,
            v.sha256
        );
    }
    Ok(())
}
