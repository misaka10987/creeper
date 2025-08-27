use std::{collections::HashMap, path::PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::Artifact;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Package {
    pub name: String,
    pub version: Version,
    #[serde(rename = "description")]
    pub desc: String,
    #[serde(rename = "dependencies")]
    pub deps: HashMap<String, Version>,
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
