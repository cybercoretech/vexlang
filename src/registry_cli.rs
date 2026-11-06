//! `vexlang login | publish | add`: argument parsing on top of `vex_registry`.
//!
//! Secrets are never required on the command line: passwords come from
//! `--password-stdin` or `VEX_REGISTRY_PASSWORD`, API keys from `--key`,
//! `--key-stdin` or `VEX_REGISTRY_TOKEN`.

use std::io::BufRead;
use std::path::PathBuf;

pub const PASSWORD_ENV: &str = "VEX_REGISTRY_PASSWORD";
pub const TOKEN_ENV: &str = "VEX_REGISTRY_TOKEN";

pub const USAGE: &str = "registry:\n\
       vexlang login --email EMAIL (--password-stdin | env VEX_REGISTRY_PASSWORD) [--registry URL]\n\
       vexlang login (--key KEY | --key-stdin | env VEX_REGISTRY_TOKEN) [--registry URL]\n\
       vexlang publish --name NAME --version VER --file FILE.tar.gz [--description TEXT]\n\
       vexlang add --name NAME --version VER [--out DIR] [--registry URL]\n\
       (registry defaults to $VEX_REGISTRY, then http://localhost:3001)";

/// Runs a registry subcommand. Returns `None` if `command` is not one, so the
/// caller can fall through to the language commands.
pub fn run(command: &str, rest: &[String]) -> Option<i32> {
    let result = match command {
        "login" => login(rest),
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

/// Parsed `--flag value` / `--switch` arguments.
struct Flags {
    values: Vec<(String, String)>,
    switches: Vec<String>,
}

impl Flags {
    fn parse(rest: &[String], valued: &[&str], switches: &[&str]) -> Result<Self, CliError> {
        let mut flags = Flags {
            values: Vec::new(),
            switches: Vec::new(),
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
            } else if arg == "--password" {
                return Err(CliError::Usage(
                    "--password is not accepted (it leaks into shell history); \
                     use --password-stdin or VEX_REGISTRY_PASSWORD"
                        .into(),
                ));
            } else {
                return Err(CliError::Usage(format!("unexpected argument `{arg}`")));
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
}

fn read_stdin_line(what: &str) -> Result<String, CliError> {
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| CliError::Failed(anyhow::anyhow!("cannot read {what} from stdin: {e}")))?;
    let line = line.trim_end_matches(['\r', '\n']).to_string();
    if line.is_empty() {
        return Err(CliError::Usage(format!("no {what} on stdin")));
    }
    Ok(line)
}

fn env_secret(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn registry_flag(flags: &Flags) -> String {
    flags
        .get("--registry")
        .map(str::to_string)
        .unwrap_or_else(vex_registry::default_registry)
}

fn login(rest: &[String]) -> CliResult {
    let flags = Flags::parse(
        rest,
        &["--registry", "--email", "--key"],
        &["--password-stdin", "--key-stdin"],
    )?;
    let registry = registry_flag(&flags);

    let creds = if let Some(email) = flags.get("--email") {
        if flags.get("--key").is_some() || flags.has("--key-stdin") {
            return Err(CliError::Usage(
                "use either --email or an API key, not both".into(),
            ));
        }
        let password = if flags.has("--password-stdin") {
            read_stdin_line("password")?
        } else {
            env_secret(PASSWORD_ENV).ok_or_else(|| {
                CliError::Usage(format!(
                    "login --email needs --password-stdin or {PASSWORD_ENV}"
                ))
            })?
        };
        vex_registry::login(&registry, email, &password)?
    } else {
        let key = if let Some(key) = flags.get("--key") {
            key.to_string()
        } else if flags.has("--key-stdin") {
            read_stdin_line("API key")?
        } else {
            env_secret(TOKEN_ENV).ok_or_else(|| {
                CliError::Usage(format!(
                    "login requires --email, --key, --key-stdin or {TOKEN_ENV}"
                ))
            })?
        };
        vex_registry::login_api_key(&registry, &key)?
    };
    println!(
        "logged in to {} (credentials in {})",
        creds.registry,
        vex_registry::credentials_path().display()
    );
    Ok(())
}

fn publish(rest: &[String]) -> CliResult {
    let flags = Flags::parse(
        rest,
        &["--name", "--version", "--file", "--description"],
        &[],
    )?;
    let name = flags.require("--name", "publish")?;
    let version = flags.require("--version", "publish")?;
    let file = PathBuf::from(flags.require("--file", "publish")?);
    let description = flags.get("--description").unwrap_or("");

    let creds = vex_registry::load_credentials()?;
    vex_registry::ensure_package(&creds, name, description)?;
    let resp = vex_registry::publish_tarball(&creds, name, version, &file)?;
    println!(
        "published {}@{} to {} ({}, {} bytes)",
        resp.package, resp.version, creds.registry, resp.checksum, resp.size
    );
    Ok(())
}

fn add(rest: &[String]) -> CliResult {
    let flags = Flags::parse(rest, &["--name", "--version", "--out", "--registry"], &[])?;
    let name = flags.require("--name", "add")?;
    let version = flags.require("--version", "add")?;
    let out = PathBuf::from(flags.get("--out").unwrap_or("."));

    // Saved credentials are only sent to the registry they belong to.
    let saved = vex_registry::load_credentials().ok();
    let registry = match (flags.get("--registry"), &saved) {
        (Some(url), _) => vex_registry::normalize_registry(url)?,
        (None, Some(creds)) => creds.registry.clone(),
        (None, None) => vex_registry::normalize_registry(&vex_registry::default_registry())?,
    };
    let token = saved
        .as_ref()
        .filter(|creds| creds.registry == registry)
        .map(|creds| creds.token.as_str());

    let dest = vex_registry::download_package(&registry, token, name, version, &out)?;
    println!("downloaded {name}@{version} to {}", dest.display());
    Ok(())
}
