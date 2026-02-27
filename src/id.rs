use std::{
    fmt::Display,
    iter::repeat,
    ops::Deref,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{anyhow, bail};
use serde_with::{DeserializeFromStr, SerializeDisplay};

/// A package identifier.
///
/// # Format
///
/// A valid package identifier is a non-empty ascii string that
///
/// - starts with a lowercase letter `a-z` ; and
///
/// - consists only of lowercase letters `a-z` , digits `0-9` , hyphens `-` , and underscores `_` .
#[derive(Clone, Debug, PartialEq, Eq, Hash, SerializeDisplay, DeserializeFromStr)]
pub struct Id(String);

impl Id {
    /// **Relative** storage path of this package to the storage root,
    /// sparsely indexed by the initial characters.
    pub fn indexed_path(&self) -> impl AsRef<Path> {
        let head4 = self
            .chars()
            .filter(char::is_ascii_lowercase)
            .chain(repeat('x'))
            .take(4)
            .collect::<String>();
        PathBuf::from(".")
            .join(&head4[0..2])
            .join(&head4[2..4])
            .join(&self.as_str())
    }

    pub fn minecraft() -> Self {
        "minecraft".parse().unwrap()
    }

    pub fn vanilla() -> Self {
        "vanilla".parse().unwrap()
    }

    pub fn forge() -> Self {
        "forge".parse().unwrap()
    }

    pub fn neoforge() -> Self {
        "neoforge".parse().unwrap()
    }

    pub fn fabric() -> Self {
        "fabric".parse().unwrap()
    }
}

impl Deref for Id {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromStr for Id {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();

        // non-empty
        let first = chars.next().ok_or(anyhow!("must not be empty"))?;

        // start with lowercase letter
        if !first.is_ascii_lowercase() {
            bail!("must start with lowercase letter");
        }

        // consist of valid characters
        for c in chars {
            if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
                bail!("invalid character {c}");
            }
        }

        Ok(Id(s.to_string()))
    }
}

impl Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}
