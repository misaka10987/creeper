use std::{collections::HashMap, path::PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::Artifact;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub name: String,
    pub version: Version,
    #[serde(rename = "description")]
    pub desc: String,
    #[serde(rename = "dependencies")]
    pub deps: HashMap<String, Version>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Install {
    #[serde(default, skip_serializing_if = "FileMap::is_empty")]
    pub java_lib: FileMap,
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

pub trait Pack {}

pub type FileMap = HashMap<PathBuf, Artifact>;
