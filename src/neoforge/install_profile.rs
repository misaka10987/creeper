use std::fmt::Display;
use std::{collections::HashMap, path::PathBuf};

use mc_launchermeta::version::library::Library;
use semver::Version;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct NfInstallProfile {
    pub spec: u64,
    pub profile: String,
    pub version: String,
    pub icon: String,
    pub minecraft: Version,
    pub json: PathBuf,
    pub logo: PathBuf,
    pub welcome: String,
    pub mirror_list: Url,
    pub hide_extract: bool,
    pub data: HashMap<String, DataValue>,
    pub processors: Vec<Processor>,
    pub libraries: Vec<Library>,
    pub server_jar_path: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct DataValue {
    pub client: String,
    pub server: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Processor {
    pub sides: Option<Vec<String>>,
    pub jar: String,
    pub classpath: Vec<String>,
    pub args: Vec<String>,
}

impl Display for Processor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.jar, self.args.join(" "))
    }
}
