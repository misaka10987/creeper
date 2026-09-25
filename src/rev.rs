use std::{
    fmt::{Debug, Display},
    str::FromStr,
};

use anyhow::bail;
use semver::Version;
use serde_with::{DeserializeFromStr, SerializeDisplay};

/// A "revisioned" version, which is a regular version with an additional revision number.
///
/// This is used to modify package definitions without changing the version number (which should correspond to the upstream),
/// while still allowing package version locking.
///
/// # Representation
///
/// A `VersionRev` will be displayed as a semver followed by a revision number,
/// separated with a hash `#`.
/// E.g. `1.2.3#4`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, SerializeDisplay, DeserializeFromStr)]
pub struct VersionRev {
    /// The version number.
    pub version: Version,

    /// The revision number, monotonically increasing and defaults to 0.
    pub rev: u32,
}

impl VersionRev {
    pub fn new(version: Version) -> Self {
        Self { version, rev: 0 }
    }

    pub fn with_rev(version: Version, rev: u32) -> Self {
        Self { version, rev }
    }
}

// note that `From<Version>` is implemented because the conversion is lossless,
// while the reverse is not
impl From<Version> for VersionRev {
    fn from(value: Version) -> Self {
        Self::new(value)
    }
}

impl Display for VersionRev {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.rev == 0 {
            return write!(f, "{}", self.version);
        }

        write!(f, "{}#{}", self.version, self.rev)
    }
}

impl Debug for VersionRev {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl FromStr for VersionRev {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let split = s.split("#").collect::<Vec<_>>();

        match split.len() {
            1 => Ok(Self::new(split[0].parse()?)),
            2 => Ok(Self::with_rev(split[0].parse()?, split[1].parse()?)),
            _ => bail!("invalid revisioned version {s}"),
        }
    }
}
