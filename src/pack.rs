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

pub struct Install {
    pub java_flags: Option<Vec<String>>,
    pub mod_file: Option<Vec<String>>,
}

pub trait Pack {}

pub type FileMap = HashMap<PathBuf, Artifact>;
