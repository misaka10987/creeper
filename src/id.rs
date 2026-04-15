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
/// - starts with a lowercase letter `a-z`; and
///
/// - consists only of lowercase letters `a-z`, digits `0-9`, hyphens `-`, and underscores `_`; and
///
/// - does not end with a hyphen `-` or underscore `_`.
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

    /// Used as the root placeholder package during dependency resolution.
    ///
    /// Intentionally contains invalid characters so that can only be constructed via this method.
    pub fn root() -> Self {
        Self("<root>".to_string())
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

    /// Whether this is a regular package or a package to be specially handled by package manager.
    pub fn is_regular(&self) -> bool {
        const SPECIAL: [&str; 6] = [
            "root",
            "minecraft",
            "vanilla",
            "forge",
            "neoforge",
            "fabric",
        ];
        !SPECIAL.contains(&self.as_str())
    }

    pub fn is_valid_index_lv1(name: &str) -> bool {
        if name.len() != 2 {
            return false;
        }
        let mut chars = name.chars();
        if !chars.next().unwrap().is_ascii_lowercase() {
            return false;
        }
        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    }

    pub fn is_valid_index_lv2(name: &str) -> bool {
        if name.len() != 2 {
            return false;
        }
        name.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    }

    pub fn is_of_index(&self, lv1: &str, lv2: &str) -> bool {
        let head4 = self
            .chars()
            .filter(char::is_ascii_lowercase)
            .chain(repeat('x'))
            .take(4)
            .collect::<String>();
        &head4[0..2] == lv1 && &head4[2..4] == lv2
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
        if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
            bail!("must consist only of lowercase letters, digits, hyphens, and underscores");
        }

        // does not end with hyphen or underscore
        if s.ends_with('-') || s.ends_with('_') {
            bail!("must not end with hyphen or underscore");
        }

        Ok(Id(s.to_string()))
    }
}

impl Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
