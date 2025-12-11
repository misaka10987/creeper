use std::{collections::HashMap, path::PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};
use spdx::Expression;

use crate::{Artifact, Id};

/// Package metadata of a specific version of a specific package.
#[serde_as]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Package {
    /// Display name of the package. Does not need to follow the specifications of package IDs.
    pub name: String,
    /// Authors of the package.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    /// A short description of this package.
    #[serde(
        default,
        rename = "description",
        skip_serializing_if = "String::is_empty"
    )]
    pub desc: String,
    /// Dependencies of this package.
    #[serde(
        default,
        rename = "dependencies",
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub deps: HashMap<Id, Version>,
    /// License of this package in [SPDX expression](https://spdx.github.io/spdx-spec/v2.3/SPDX-license-expressions/) format.
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<Expression>,
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
