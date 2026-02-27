use std::fmt::{Debug, Display};

use pubgrub::Ranges;
use semver::{Version, VersionReq};

pub struct Error(anyhow::Error);

impl From<anyhow::Error> for Error {
    fn from(value: anyhow::Error) -> Self {
        Self(value)
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

fn ranges_exact(major: u64, minor: Option<u64>, patch: Option<u64>) -> Ranges<Version> {
    match (minor, patch) {
        (None, None) => ranges_greater_eq(major, Some(0), Some(0)).intersection(&ranges_less(
            major + 1,
            Some(0),
            Some(0),
        )),
        (None, Some(_)) => panic!("invalid version requirement"),
        (Some(minor), None) => ranges_greater_eq(major, Some(minor), Some(0))
            .intersection(&ranges_less(major, Some(minor + 1), Some(0))),
        (Some(minor), Some(patch)) => Ranges::singleton(Version::new(major, minor, patch)),
    }
}

fn ranges_greater(major: u64, minor: Option<u64>, patch: Option<u64>) -> Ranges<Version> {
    match (minor, patch) {
        (None, None) => ranges_greater_eq(major + 1, Some(0), Some(0)),
        (None, Some(_)) => panic!("invalid version requirement"),
        (Some(minor), None) => ranges_greater_eq(major, Some(minor + 1), Some(0)),
        (Some(minor), Some(patch)) => {
            Ranges::strictly_higher_than(Version::new(major, minor, patch))
        }
    }
}

fn ranges_greater_eq(major: u64, minor: Option<u64>, patch: Option<u64>) -> Ranges<Version> {
    Ranges::higher_than(Version::new(major, minor.unwrap_or(0), patch.unwrap_or(0)))
}

fn ranges_less(major: u64, minor: Option<u64>, patch: Option<u64>) -> Ranges<Version> {
    Ranges::strictly_lower_than(Version::new(major, minor.unwrap_or(0), patch.unwrap_or(0)))
}

fn ranges_less_eq(major: u64, minor: Option<u64>, patch: Option<u64>) -> Ranges<Version> {
    match (minor, patch) {
        (None, None) => ranges_less(major + 1, Some(0), Some(0)),
        (None, Some(_)) => panic!("invalid version requirement"),
        (Some(minor), None) => ranges_less(major, Some(minor + 1), Some(0)),
        (Some(minor), Some(patch)) => Ranges::lower_than(Version::new(major, minor, patch)),
    }
}

fn ranges_tilde(major: u64, minor: Option<u64>, patch: Option<u64>) -> Ranges<Version> {
    match (minor, patch) {
        (None, None) => ranges_exact(major, None, None),
        (None, Some(_)) => panic!("invalid version requirement"),
        (Some(minor), None) => ranges_exact(major, Some(minor), None),
        (Some(minor), Some(patch)) => ranges_greater_eq(major, Some(minor), Some(patch))
            .intersection(&ranges_less(major, Some(minor + 1), Some(0))),
    }
}

fn ranges_caret(major: u64, minor: Option<u64>, patch: Option<u64>) -> Ranges<Version> {
    match (major, minor, patch) {
        (major, Some(minor), Some(patch)) if major > 0 => ranges_greater_eq(
            major,
            Some(minor),
            Some(patch),
        )
        .intersection(&ranges_less(major + 1, Some(0), Some(0))),

        (0, Some(minor), Some(patch)) if minor > 0 => ranges_greater_eq(
            0,
            Some(minor),
            Some(patch),
        )
        .intersection(&ranges_less(0, Some(minor + 1), Some(0))),

        (0, Some(0), Some(patch)) => ranges_exact(0, Some(0), Some(patch)),

        (major, Some(minor), None) if major > 0 || minor > 0 => {
            ranges_caret(major, Some(minor), Some(0))
        }

        (0, Some(0), None) => ranges_exact(0, Some(0), None),

        (major, None, None) => ranges_exact(major, None, None),

        (_, _, _) => unreachable!(),
    }
}

fn ranges_wildcard(major: u64, minor: Option<u64>, patch: Option<u64>) -> Ranges<Version> {
    match minor {
        Some(minor) => ranges_exact(major, Some(minor), None),
        None => ranges_exact(major, None, None),
    }
}

pub fn ranges_for(req: VersionReq) -> anyhow::Result<Ranges<Version>> {
    let mut res = Ranges::full();
    for cmp in req.comparators {
        let new = match cmp.op {
            semver::Op::Exact => ranges_exact(cmp.major, cmp.minor, cmp.patch),
            semver::Op::Greater => ranges_greater(cmp.major, cmp.minor, cmp.patch),
            semver::Op::GreaterEq => ranges_greater_eq(cmp.major, cmp.minor, cmp.patch),
            semver::Op::Less => ranges_less(cmp.major, cmp.minor, cmp.patch),
            semver::Op::LessEq => ranges_less_eq(cmp.major, cmp.minor, cmp.patch),
            semver::Op::Tilde => ranges_tilde(cmp.major, cmp.minor, cmp.patch),
            semver::Op::Caret => ranges_caret(cmp.major, cmp.minor, cmp.patch),
            semver::Op::Wildcard => ranges_wildcard(cmp.major, cmp.minor, cmp.patch),
            _ => todo!(),
        };
        res = res.intersection(&new);
    }
    Ok(res)
}
