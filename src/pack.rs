use std::{
    collections::HashMap, fmt::Display, iter::repeat, ops::Deref, path::PathBuf, str::FromStr,
};

use anyhow::{anyhow, bail};
use semver::VersionReq;
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};

use crate::Artifact;

#[derive(Clone, Debug, PartialEq, Eq, Hash, SerializeDisplay, DeserializeFromStr)]
pub struct Id(String);

impl Id {
    pub fn path(&self) -> PathBuf {
        let head4 = self
            .chars()
            .filter(char::is_ascii_lowercase)
            .chain(repeat('x'))
            .take(4)
            .collect::<String>();
        let path = format!("./{}/{}/{self}", &head4[0..2], &head4[2..4]);
        path.into()
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Package {
    pub version: HashMap<Id, PackageVersion>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PackageVersion {
    pub name: String,
    #[serde(rename = "description")]
    pub desc: String,
    #[serde(rename = "dependencies")]
    pub deps: HashMap<Id, VersionReq>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Install {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub java_lib: Vec<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_main_class: Option<String>,
    #[serde(default, skip_serializing_if = "FileMap::is_empty")]
    pub native: FileMap,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub java_flag: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mc_jar: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mc_flag: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mc_asset_index: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mc_mod: Vec<Artifact>,
}

impl Install {
    pub fn merge(self, next: Self) -> Self {
        let mut new = self;
        new.extend(Some(next));
        new
    }
}

impl Extend<Self> for Install {
    fn extend<T: IntoIterator<Item = Self>>(&mut self, iter: T) {
        for next in iter {
            let Self {
                java_lib,
                java_main_class,
                native,
                java_flag,
                mc_jar,
                mc_flag,
                mc_asset_index,
                mc_mod,
            } = next;
            self.java_lib.extend(java_lib);
            self.java_main_class = self.java_main_class.take().or(java_main_class);
            self.native.extend(native);
            self.java_flag.extend(java_flag);
            self.mc_jar = self.mc_jar.take().or(mc_jar);
            self.mc_flag.extend(mc_flag);
            self.mc_asset_index = self.mc_asset_index.take().or(mc_asset_index);
            self.mc_mod.extend(mc_mod);
        }
    }
}

pub type FileMap = HashMap<PathBuf, Artifact>;
