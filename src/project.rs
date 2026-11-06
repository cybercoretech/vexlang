//! `vex.toml` project manifests.
//!
//! ```toml
//! [project]
//! name = "demo"
//! version = "0.1.0"
//! source = "src/main.vex"      # program entry (default src/main.vex)
//! lib = "src/lib.vex"          # what `use demo;` loads (default src/lib.vex)
//! description = "One line"
//!
//! [dependencies]
//! mathx = "0.2.0"              # exact versions
//! ```
//!
//! The `[project]` header is optional for backwards compatibility. Only this
//! small subset of TOML is accepted: one `key = "value"` per line.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectConfig {
    pub name: String,
    pub version: String,
    pub source: String,
    pub lib: String,
    pub description: Option<String>,
    pub dependencies: BTreeMap<String, String>,
}

pub const DEFAULT_SOURCE: &str = "src/main.vex";
pub const DEFAULT_LIB: &str = "src/lib.vex";

pub fn load(path: &Path) -> Result<ProjectConfig, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("could not read `{}`: {error}", path.display()))?;
    parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Project,
    Dependencies,
}

fn section_of(line: &str) -> Option<Result<Section, String>> {
    let header = line.strip_prefix('[')?.strip_suffix(']')?.trim();
    Some(match header {
        "project" => Ok(Section::Project),
        "dependencies" => Ok(Section::Dependencies),
        other => Err(format!("unknown section `[{other}]`")),
    })
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with(['.', '-'])
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
}

pub fn parse(text: &str) -> Result<ProjectConfig, String> {
    let mut section = Section::Project;
    let mut name = None;
    let mut version = None;
    let mut source = None;
    let mut lib = None;
    let mut description = None;
    let mut dependencies = BTreeMap::new();
    for (number, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(next) = section_of(line) {
            section = next.map_err(|e| format!("line {}: {e}", number + 1))?;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("invalid project configuration line `{line}`"));
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').to_string();
        match section {
            Section::Project => match key {
                "name" => name = Some(value),
                "version" => version = Some(value),
                "source" => source = Some(value),
                "lib" => lib = Some(value),
                "description" => description = Some(value),
                other => return Err(format!("unknown project configuration key `{other}`")),
            },
            Section::Dependencies => {
                if !valid_component(key) {
                    return Err(format!("invalid dependency name `{key}`"));
                }
                if !valid_component(&value) {
                    return Err(format!("invalid version `{value}` for dependency `{key}`"));
                }
                if dependencies.insert(key.to_string(), value).is_some() {
                    return Err(format!("dependency `{key}` is listed twice"));
                }
            }
        }
    }
    Ok(ProjectConfig {
        name: name.ok_or("project configuration is missing `name`")?,
        version: version.ok_or("project configuration is missing `version`")?,
        source: source.unwrap_or_else(|| DEFAULT_SOURCE.into()),
        lib: lib.unwrap_or_else(|| DEFAULT_LIB.into()),
        description,
        dependencies,
    })
}

/// Returns `text` with `name = "version"` set in `[dependencies]`, keeping
/// every other line (comments included) as it was. Adds the section if needed.
pub fn set_dependency(text: &str, name: &str, version: &str) -> String {
    let entry = format!("{name} = \"{version}\"");
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut in_deps = false;
    let mut deps_end = None;
    for (i, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim();
        if let Some(section) = section_of(trimmed) {
            in_deps = matches!(section, Ok(Section::Dependencies));
            if in_deps {
                deps_end = Some(i + 1);
            }
            continue;
        }
        if in_deps {
            if let Some((key, _)) = trimmed.split_once('=') {
                if key.trim() == name {
                    *line = entry;
                    return finish(lines);
                }
            }
            if !trimmed.is_empty() {
                deps_end = Some(i + 1);
            }
        }
    }
    match deps_end {
        Some(at) => lines.insert(at, entry),
        None => {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push("[dependencies]".into());
            lines.push(entry);
        }
    }
    finish(lines)
}

/// Returns `text` without `name` in `[dependencies]`, or `None` if it was
/// not listed.
pub fn remove_dependency(text: &str, name: &str) -> Option<String> {
    let mut in_deps = false;
    let mut removed = false;
    let lines: Vec<String> = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if let Some(section) = section_of(trimmed) {
                in_deps = matches!(section, Ok(Section::Dependencies));
                return true;
            }
            if in_deps {
                if let Some((key, _)) = trimmed.split_once('=') {
                    if key.trim() == name {
                        removed = true;
                        return false;
                    }
                }
            }
            true
        })
        .map(str::to_string)
        .collect();
    removed.then(|| finish(lines))
}

fn finish(lines: Vec<String>) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_headerless_manifest() {
        let config = parse("name = \"demo\"\nversion = \"0.1.0\"\n").unwrap();
        assert_eq!(config.source, DEFAULT_SOURCE);
        assert_eq!(config.lib, DEFAULT_LIB);
        assert!(config.dependencies.is_empty());
    }

    #[test]
    fn parses_sections_and_dependencies() {
        let config = parse(
            "# demo\n[project]\nname = \"demo\"\nversion = \"0.1.0\"\nlib = \"lib.vex\"\n\
             description = \"Demo\"\n\n[dependencies]\nmathx = \"0.2.0\"\nstrs = \"1.0.0\"\n",
        )
        .unwrap();
        assert_eq!(config.lib, "lib.vex");
        assert_eq!(config.description.as_deref(), Some("Demo"));
        assert_eq!(config.dependencies["mathx"], "0.2.0");
        assert_eq!(config.dependencies.len(), 2);
    }

    #[test]
    fn rejects_bad_dependencies_and_sections() {
        let base = "name = \"d\"\nversion = \"1\"\n";
        for bad in [
            "[dependencies]\n../x = \"1.0.0\"\n",
            "[dependencies]\nx = \"../1\"\n",
            "[dependencies]\nx = \"1\"\nx = \"2\"\n",
            "[dev-dependencies]\n",
        ] {
            assert!(parse(&format!("{base}{bad}")).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn set_dependency_adds_section_updates_and_preserves_comments() {
        let text = "# keep me\nname = \"d\"\nversion = \"1\"\n";
        let added = set_dependency(text, "mathx", "0.1.0");
        assert_eq!(
            added,
            "# keep me\nname = \"d\"\nversion = \"1\"\n\n[dependencies]\nmathx = \"0.1.0\"\n"
        );
        let second = set_dependency(&added, "strs", "2.0.0");
        assert!(
            second.ends_with("mathx = \"0.1.0\"\nstrs = \"2.0.0\"\n"),
            "{second}"
        );
        let bumped = set_dependency(&second, "mathx", "0.2.0");
        assert!(bumped.contains("mathx = \"0.2.0\"") && !bumped.contains("0.1.0"));
        assert_eq!(parse(&bumped).unwrap().dependencies.len(), 2);
    }

    #[test]
    fn set_dependency_inserts_before_a_following_section() {
        let text = "[dependencies]\na = \"1\"\n\n[project]\nname = \"d\"\nversion = \"1\"\n";
        let out = set_dependency(text, "b", "2");
        assert!(
            out.starts_with("[dependencies]\na = \"1\"\nb = \"2\"\n"),
            "{out}"
        );
        assert_eq!(parse(&out).unwrap().dependencies.len(), 2);
    }

    #[test]
    fn remove_dependency_only_touches_dependencies() {
        let text = "name = \"mathx\"\nversion = \"1\"\n[dependencies]\nmathx = \"1\"\nb = \"2\"\n";
        let out = remove_dependency(text, "mathx").unwrap();
        assert_eq!(
            out,
            "name = \"mathx\"\nversion = \"1\"\n[dependencies]\nb = \"2\"\n"
        );
        assert!(remove_dependency(&out, "zzz").is_none());
    }
}
