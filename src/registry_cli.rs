//! `vexlang package | publish | add`: argument parsing on top of `vex_registry`.
//!
//! Packages live in a static Git index (no server, no login). See the
//! `vex_registry` crate docs for the index format.

use std::path::{Path, PathBuf};

pub const USAGE: &str = "packages:\n\
       vexlang package [vex.toml] [--out DIR]           build dist/NAME-VERSION.tar.gz\n\
       vexlang publish [vex.toml] --index DIR --url URL [--file TARBALL] [--description TEXT]\n\
                                                       record the version in an index checkout\n\
       vexlang add NAME@VERSION [--out DIR] [--index URL|PATH]\n\
                                                       download and verify a package\n\
       (index defaults to $VEX_INDEX, then the cybercoretech/vex-index repository)";

/// Runs a package subcommand. Returns `None` if `command` is not one, so the
/// caller can fall through to the language commands.
pub fn run(command: &str, rest: &[String]) -> Option<i32> {
    let result = match command {
        "package" => package(rest),
        "publish" => publish(rest),
        "add" => add(rest),
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

type CliResult = Result<(), CliError>;

/// Parsed `--flag value` pairs plus at most one positional argument.
struct Flags {
    values: Vec<(String, String)>,
    positional: Option<String>,
}

impl Flags {
    fn parse(rest: &[String], valued: &[&str]) -> Result<Self, CliError> {
        let mut flags = Flags {
            values: Vec::new(),
            positional: None,
        };
        let mut iter = rest.iter();
        while let Some(arg) = iter.next() {
            if valued.contains(&arg.as_str()) {
                let value = iter
                    .next()
                    .ok_or_else(|| CliError::Usage(format!("{arg} needs a value")))?;
                flags.values.push((arg.clone(), value.clone()));
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
}

/// Loads `vex.toml` (or the given manifest) and returns it with its directory.
fn manifest(flags: &Flags) -> Result<(PathBuf, PathBuf, crate::project::ProjectConfig), CliError> {
    let manifest = PathBuf::from(flags.positional.as_deref().unwrap_or("vex.toml"));
    let config =
        crate::project::load(&manifest).map_err(|e| CliError::Failed(anyhow::anyhow!(e)))?;
    let dir = manifest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf();
    Ok((manifest, dir, config))
}

fn package(rest: &[String]) -> CliResult {
    let flags = Flags::parse(rest, &["--out"])?;
    let (manifest, dir, config) = manifest(&flags)?;
    let out = flags
        .get("--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join("dist"));
    let (tarball, files) =
        vex_registry::package(&dir, &manifest, &config.name, &config.version, &out)?;
    for file in &files {
        println!("  {}", file.display());
    }
    println!(
        "packaged {}@{} -> {} (sha256 {})",
        config.name,
        config.version,
        tarball.display(),
        vex_registry::sha256_file(&tarball)?
    );
    Ok(())
}

fn publish(rest: &[String]) -> CliResult {
    let flags = Flags::parse(rest, &["--index", "--url", "--file", "--description"])?;
    let (_, dir, config) = manifest(&flags)?;
    let index_dir = PathBuf::from(flags.require("--index", "publish")?);
    let url = flags.require("--url", "publish")?;
    let tarball = flags.get("--file").map(PathBuf::from).unwrap_or_else(|| {
        dir.join("dist")
            .join(format!("{}-{}.tar.gz", config.name, config.version))
    });

    let published = vex_registry::publish(
        &index_dir,
        &config.name,
        &config.version,
        &tarball,
        url,
        flags.get("--description"),
    )?;
    println!(
        "recorded {}@{} in {} (sha256 {}, {} bytes)",
        config.name,
        config.version,
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

fn add(rest: &[String]) -> CliResult {
    let flags = Flags::parse(rest, &["--out", "--index"])?;
    let spec = flags
        .positional
        .as_deref()
        .ok_or_else(|| CliError::Usage("add requires NAME@VERSION".into()))?;
    let (name, version) = spec
        .split_once('@')
        .ok_or_else(|| CliError::Usage(format!("expected NAME@VERSION, got `{spec}`")))?;
    let out = PathBuf::from(flags.get("--out").unwrap_or("."));
    let location = flags
        .get("--index")
        .map(str::to_string)
        .unwrap_or_else(vex_registry::default_index);

    let index = vex_registry::load_index(&location)?;
    let dest = vex_registry::add(&index, name, version, &out)?;
    println!(
        "added {name}@{version} -> {} (sha256 verified)",
        dest.display()
    );
    Ok(())
}
