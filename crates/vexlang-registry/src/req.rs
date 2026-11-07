//! Version requirements, following Cargo's conventions.
//!
//! | Requirement | Matches |
//! | --- | --- |
//! | `1.2.3`, `^1.2.3` | `>=1.2.3, <2.0.0` |
//! | `^0.2.3` | `>=0.2.3, <0.3.0` (for `0.x`, the minor version is the breaking one) |
//! | `^0.0.3` | `>=0.0.3, <0.0.4` |
//! | `~1.2.3` | `>=1.2.3, <1.3.0` |
//! | `=1.2.3` | exactly `1.2.3` |
//! | `1.2.*`, `1.*`, `*` | any version with that prefix |
//! | `>=1.2, <1.5` | every comparator must match (`>`, `>=`, `<`, `<=`, `=`) |
//!
//! Missing minor or patch numbers are zero for lower bounds (`^1.2` is
//! `>=1.2.0, <2.0.0`). A pre-release such as `1.3.0-rc.1` only matches a
//! requirement that names a pre-release of the same `1.3.0`, so ranges never
//! pick up pre-releases by accident.

use anyhow::{Result, bail};
use std::cmp::Ordering;
use std::fmt;

/// A version as written in the index: `MAJOR.MINOR.PATCH[-PRE][+BUILD]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl Version {
    /// Parses a full version. Build metadata (`+...`) is accepted and ignored.
    pub fn parse(text: &str) -> Result<Version> {
        let core = text.split('+').next().unwrap_or(text);
        let (core, pre) = match core.split_once('-') {
            Some((core, pre)) if !pre.is_empty() => (core, Some(pre.to_string())),
            Some(_) => bail!("invalid version `{text}`"),
            None => (core, None),
        };
        let numbers: Vec<&str> = core.split('.').collect();
        let [major, minor, patch] = numbers.as_slice() else {
            bail!("invalid version `{text}`: expected MAJOR.MINOR.PATCH");
        };
        Ok(Version {
            major: number(major, text)?,
            minor: number(minor, text)?,
            patch: number(patch, text)?,
            pre,
        })
    }

    fn core(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }

    fn release(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            major,
            minor,
            patch,
            pre: None,
        }
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        crate::compare_versions(&self.to_string(), &other.to_string())
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.pre {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

fn number(part: &str, text: &str) -> Result<u64> {
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) || part.len() > 18 {
        bail!("invalid version `{text}`: `{part}` is not a number");
    }
    if part.len() > 1 && part.starts_with('0') {
        bail!("invalid version `{text}`: `{part}` has a leading zero");
    }
    Ok(part.parse()?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Caret,
    Tilde,
    Exact,
    Greater,
    GreaterEq,
    Less,
    LessEq,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Comparator {
    op: Op,
    major: Option<u64>,
    minor: Option<u64>,
    patch: Option<u64>,
    pre: Option<String>,
}

/// A parsed dependency requirement such as `^0.1` or `>=1.2, <2`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReq {
    text: String,
    comparators: Vec<Comparator>,
}

impl VersionReq {
    pub fn parse(text: &str) -> Result<VersionReq> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            bail!("empty version requirement");
        }
        if trimmed.len() > 128 {
            bail!("version requirement `{trimmed}` is too long");
        }
        let comparators = trimmed
            .split(',')
            .map(|part| parse_comparator(part.trim(), text))
            .collect::<Result<Vec<_>>>()?;
        Ok(VersionReq {
            text: trimmed.to_string(),
            comparators,
        })
    }

    pub fn matches(&self, version: &Version) -> bool {
        if version.pre.is_some()
            && !self
                .comparators
                .iter()
                .any(|c| c.pre.is_some() && c.full_core() == Some(version.core()))
        {
            return false;
        }
        self.comparators.iter().all(|c| c.matches(version))
    }

    /// The exact version this requirement pins (`=1.2.3`), if any.
    pub fn exact(&self) -> Option<Version> {
        match self.comparators.as_slice() {
            [c] if c.op == Op::Exact => c.full_core().map(|(major, minor, patch)| Version {
                major,
                minor,
                patch,
                pre: c.pre.clone(),
            }),
            _ => None,
        }
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

fn parse_comparator(part: &str, text: &str) -> Result<Comparator> {
    let (op, rest) = [
        (">=", Op::GreaterEq),
        ("<=", Op::LessEq),
        (">", Op::Greater),
        ("<", Op::Less),
        ("=", Op::Exact),
        ("~", Op::Tilde),
        ("^", Op::Caret),
    ]
    .iter()
    .find_map(|(prefix, op)| {
        part.strip_prefix(prefix)
            .map(|rest| (*op, rest.trim_start()))
    })
    .unwrap_or((Op::Caret, part));
    if rest.is_empty() {
        bail!("invalid version requirement `{text}`");
    }
    let (core, pre) = match rest.split_once('-') {
        Some((core, pre)) if !pre.is_empty() => (core, Some(pre.to_string())),
        Some(_) => bail!("invalid version requirement `{text}`"),
        None => (rest, None),
    };
    if pre
        .as_deref()
        .is_some_and(|p| !p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.'))
    {
        bail!("invalid pre-release in version requirement `{text}`");
    }
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() > 3 {
        bail!("invalid version requirement `{text}`");
    }
    let mut numbers = [None; 3];
    let mut wildcard = false;
    for (slot, part) in numbers.iter_mut().zip(&parts) {
        if matches!(*part, "*" | "x" | "X") {
            wildcard = true;
        } else if wildcard {
            bail!("invalid version requirement `{text}`: nothing may follow `*`");
        } else {
            *slot = Some(number(part, text)?);
        }
    }
    if wildcard && (op != Op::Caret || pre.is_some() || part.starts_with('^')) {
        bail!("invalid version requirement `{text}`: `*` cannot be combined with an operator");
    }
    if pre.is_some() && parts.len() != 3 {
        bail!("invalid version requirement `{text}`: a pre-release needs MAJOR.MINOR.PATCH");
    }
    Ok(Comparator {
        // `1.2.*` is the same as `=1.2`.
        op: if wildcard { Op::Exact } else { op },
        major: numbers[0],
        minor: numbers[1],
        patch: numbers[2],
        pre,
    })
}

impl Comparator {
    fn full_core(&self) -> Option<(u64, u64, u64)> {
        Some((self.major?, self.minor?, self.patch?))
    }

    /// The smallest version this comparator's numbers describe.
    fn floor(&self) -> Version {
        Version {
            major: self.major.unwrap_or(0),
            minor: self.minor.unwrap_or(0),
            patch: self.patch.unwrap_or(0),
            pre: self.pre.clone(),
        }
    }

    /// The first release after every version with this comparator's prefix
    /// (`1.2` -> `1.3.0`, `1` -> `2.0.0`, `1.2.3` -> `1.2.4`).
    fn prefix_end(&self) -> Option<Version> {
        Some(match (self.major?, self.minor, self.patch) {
            (major, None, _) => Version::release(major + 1, 0, 0),
            (major, Some(minor), None) => Version::release(major, minor + 1, 0),
            (major, Some(minor), Some(patch)) => Version::release(major, minor, patch + 1),
        })
    }

    fn matches(&self, v: &Version) -> bool {
        let Some(major) = self.major else {
            return true; // `*`
        };
        let floor = self.floor();
        match self.op {
            Op::Exact => match self.full_core() {
                Some(_) => *v == floor,
                None => *v >= floor && self.prefix_end().is_none_or(|end| *v < end),
            },
            Op::GreaterEq => *v >= floor,
            Op::Greater => match self.full_core() {
                Some(_) => *v > floor,
                None => self.prefix_end().is_none_or(|end| *v >= end),
            },
            Op::Less => *v < floor,
            Op::LessEq => match self.full_core() {
                Some(_) => *v <= floor,
                None => self.prefix_end().is_none_or(|end| *v < end),
            },
            Op::Tilde => {
                let end = match self.minor {
                    Some(minor) => Version::release(major, minor + 1, 0),
                    None => Version::release(major + 1, 0, 0),
                };
                *v >= floor && *v < end
            }
            Op::Caret => {
                let end = match (major, self.minor, self.patch) {
                    (0, None, _) => Version::release(1, 0, 0),
                    (0, Some(0), None) => Version::release(0, 1, 0),
                    (0, Some(0), Some(patch)) => Version::release(0, 0, patch + 1),
                    (0, Some(minor), _) => Version::release(0, minor + 1, 0),
                    (major, _, _) => Version::release(major + 1, 0, 0),
                };
                *v >= floor && *v < end
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(req: &str, yes: &[&str], no: &[&str]) {
        let parsed = VersionReq::parse(req).unwrap();
        for v in yes {
            assert!(
                parsed.matches(&Version::parse(v).unwrap()),
                "{req} should match {v}"
            );
        }
        for v in no {
            assert!(
                !parsed.matches(&Version::parse(v).unwrap()),
                "{req} should not match {v}"
            );
        }
    }

    #[test]
    fn caret_is_the_default() {
        check("1.2.3", &["1.2.3", "1.9.0"], &["1.2.2", "2.0.0"]);
        check("^1.2", &["1.2.0", "1.99.1"], &["1.1.9", "2.0.0"]);
        check("^1", &["1.0.0", "1.5.0"], &["0.9.0", "2.0.0"]);
        check("0.1.0", &["0.1.0", "0.1.7"], &["0.2.0", "0.0.9"]);
        check("^0.2.3", &["0.2.3", "0.2.9"], &["0.2.2", "0.3.0"]);
        check("^0.0.3", &["0.0.3"], &["0.0.4", "0.0.2"]);
        check("^0.0", &["0.0.0", "0.0.9"], &["0.1.0"]);
        check("^0", &["0.0.1", "0.9.9"], &["1.0.0"]);
    }

    #[test]
    fn tilde_exact_wildcards_and_comparisons() {
        check("~1.2.3", &["1.2.3", "1.2.9"], &["1.3.0", "1.2.2"]);
        check("~1.2", &["1.2.0", "1.2.9"], &["1.3.0"]);
        check("~1", &["1.0.0", "1.9.0"], &["2.0.0"]);
        check("=1.2.3", &["1.2.3"], &["1.2.4", "1.2.2"]);
        check("=1.2", &["1.2.0", "1.2.7"], &["1.3.0"]);
        check("1.2.*", &["1.2.0", "1.2.9"], &["1.3.0", "1.1.0"]);
        check("1.*", &["1.0.0", "1.9.0"], &["2.0.0"]);
        check("*", &["0.0.1", "9.9.9"], &["1.0.0-rc.1"]);
        check(">=1.2, <1.5", &["1.2.0", "1.4.9"], &["1.1.9", "1.5.0"]);
        check(">1.2.3", &["1.2.4"], &["1.2.3"]);
        check(">1.2", &["1.3.0"], &["1.2.9"]);
        check("<=1.2", &["1.2.9"], &["1.3.0"]);
        check("<=1.2.3", &["1.2.3"], &["1.2.4"]);
        check("< 2", &["1.9.9"], &["2.0.0"]);
    }

    #[test]
    fn pre_releases_only_match_when_named() {
        check("^1.2.3", &[], &["1.3.0-rc.1", "1.2.4-beta"]);
        check(
            "^1.3.0-rc.1",
            &["1.3.0-rc.1", "1.3.0-rc.2", "1.3.0", "1.4.0"],
            &["1.3.0-beta", "1.4.0-rc.1"],
        );
        check(
            ">=1.0.0-alpha, <2",
            &["1.0.0-beta", "1.5.0"],
            &["1.5.0-rc.1"],
        );
        check("=1.0.0-rc.1", &["1.0.0-rc.1"], &["1.0.0"]);
    }

    #[test]
    fn rejects_malformed_requirements() {
        for bad in [
            "",
            " ",
            "^",
            ">=",
            "1.2.3.4",
            "01.2",
            "a.b",
            "1.*.3",
            "^1.*",
            ">=*",
            "1.2-rc",
            "1.2.3-",
            "1.2.3-bad!",
            "1,,2",
            "../x",
        ] {
            assert!(
                VersionReq::parse(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
        assert_eq!(
            VersionReq::parse("=1.2.3").unwrap().exact(),
            Some(Version::parse("1.2.3").unwrap())
        );
        assert_eq!(VersionReq::parse("1.2.3").unwrap().exact(), None);
    }

    #[test]
    fn parses_versions() {
        assert_eq!(
            Version::parse("1.2.3+build.5").unwrap().to_string(),
            "1.2.3"
        );
        assert_eq!(
            Version::parse("1.2.3-rc.1").unwrap().to_string(),
            "1.2.3-rc.1"
        );
        for bad in ["1.2", "1.2.3.4", "v1.2.3", "1.02.3", "1.2.3-"] {
            assert!(Version::parse(bad).is_err(), "{bad}");
        }
    }
}
