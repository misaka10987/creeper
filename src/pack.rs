use std::{collections::HashMap, path::PathBuf};

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use serde_with::{DisplayFromStr, serde_as};
use spdx::Expression;

use crate::{Artifact, Id};

/// The package node in the dependency graph, containing only metadata needed for dependency resolution.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PackNode {
    /// Dependencies.
    #[serde(
        default,
        rename = "dependencies",
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub dep: HashMap<Id, VersionReq>,
}

#[allow(unused)] // used by `#[serde(skip_serializing_if = "is_zero")]`
fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// A package definition.
#[serde_inline_default]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Package {
    /// Unique package identifier.
    /// See [`Id`] for specifications.
    pub id: Id,
    /// Version of the package.
    pub version: Version,
    /// Revision number of this version of the package.
    #[serde_inline_default(0)]
    #[serde(skip_serializing_if = "is_zero")]
    pub rev: u32,
    #[serde(flatten)]
    pub node: PackNode,
    #[serde(rename = "package")]
    pub meta: PackMeta,
    pub install: Install,
}

/// Package metadata of a specific version of a specific package.
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PackMeta {
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
    /// License of this package in [SPDX expression](https://spdx.github.io/spdx-spec/v2.3/SPDX-license-expressions/) format.
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<Expression>,
}

/// Things installed to the game instance by a package.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Install {
    /// Additional java libraries, prepended to the classpath when launching the game.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub java_lib: Vec<Artifact>,
    /// Java main class override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_main_class: Option<String>,
    /// Native libraries to be added.
    #[serde(default, skip_serializing_if = "FileMap::is_empty")]
    pub native: FileMap,
    /// Extra java command line options.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub java_flag: Vec<String>,
    /// Minecraft client `.jar` file override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mc_jar: Option<Artifact>,
    /// Command line options passed to the Minecraft game program.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mc_flag: Vec<String>,
    /// Minecraft asset index JSON file override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mc_asset_index: Option<Artifact>,
    /// Minecraft mod files to be added to the `mods` folder.
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
