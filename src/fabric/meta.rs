use std::{collections::HashMap, path::PathBuf};

use parse_display::{Display, FromStr};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use serde_with::{DeserializeFromStr, SerializeDisplay};
use tracing::error;
use url::Url;

use crate::util::parse_or_prompt;

#[serde_as]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FabricMod {
    pub schema_version: u64,

    pub id: String,

    pub version: Version,

    // #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,

    #[serde(default, skip_serializing_if = "Contact::is_empty")]
    pub contact: Contact,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<Author>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contributors: Vec<Author>,

    // #[serde_as(as = "Option<DisplayFromStr>")]
    // #[serde(default, skip_serializing_if = "Option::is_none")]
    // pub license: Option<spdx::Expression>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,

    #[serde(default, skip_serializing_if = "is_all")]
    pub environment: Environment,

    #[serde(default, skip_serializing_if = "EntryPoints::is_empty")]
    pub entrypoints: EntryPoints,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jars: Vec<Jar>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub language_adapters: HashMap<String, String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mixins: Vec<Mixin>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_widener: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provides: Vec<String>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub depends: HashMap<String, Dependency>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub recommends: HashMap<String, Dependency>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub suggests: HashMap<String, Dependency>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub breaks: HashMap<String, Dependency>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub conflicts: HashMap<String, Dependency>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<serde_json::Value>,
}

#[allow(unused)] // used by #[serde(skip_serializing_if = "is_all")]
fn is_all(env: &Environment) -> bool {
    *env == Environment::All
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<Url>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub irc: Option<Url>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issues: Option<Url>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Url>,

    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Url>,
}

impl Contact {
    pub fn is_empty(&self) -> bool {
        self.email.is_none()
            && self.homepage.is_none()
            && self.irc.is_none()
            && self.issues.is_none()
            && self.sources.is_none()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields, rename_all = "camelCase")]
pub enum Author {
    Name(String),
    WithContact { name: String, contact: Contact },
}

impl Author {
    pub fn name(self) -> String {
        match self {
            Author::Name(name) => name,
            Author::WithContact { name, .. } => name,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields, rename_all = "camelCase")]
pub enum Icon {
    Single(PathBuf),
    Widths(HashMap<u32, PathBuf>),
}

#[derive(Clone, Copy, PartialEq, Eq, Display, FromStr, SerializeDisplay, DeserializeFromStr)]
#[display(style = "camelCase")]
pub enum Environment {
    #[display("*")]
    All,
    Client,
    Server,
}

impl Default for Environment {
    fn default() -> Self {
        Self::All
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPoints {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub main: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub client: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub server: Vec<String>,

    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Vec<String>>,
}

#[test]
fn test() {
    let json = r#"{
    "client": [
      "net.caffeinemc.mods.sodium.fabric.SodiumFabricMod"
    ],
    "preLaunch": [
      "net.caffeinemc.mods.sodium.fabric.SodiumPreLaunch"
    ]
  }"#;
    serde_json::from_str::<EntryPoints>(json).unwrap();
}

impl EntryPoints {
    pub fn is_empty(&self) -> bool {
        self.main.is_empty() && self.client.is_empty() && self.server.is_empty()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Jar {
    pub file: PathBuf,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields, rename_all = "camelCase")]
pub enum Mixin {
    Config(PathBuf),
    WithEnvironment {
        config: PathBuf,
        environment: Environment,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields, rename_all = "camelCase")]
pub enum Dependency {
    Req(VersionReq),

    List(Vec<Version>),

    VersionReq(String),

    VersionList(Vec<String>),
}

impl Dependency {
    pub async fn prompt_normalize(&self) -> anyhow::Result<VersionReq> {
        let req = match self {
            crate::fabric::meta::Dependency::Req(req) => req.clone(),
            crate::fabric::meta::Dependency::List(_) => {
                error!("does not support list of versions in fabric dependency, defaulting to *");
                VersionReq::STAR
            }
            crate::fabric::meta::Dependency::VersionReq(req) => {
                parse_or_prompt(&req, "version requirement").await?
            }
            crate::fabric::meta::Dependency::VersionList(_) => {
                error!("does not support list of versions in fabric dependency, defaulting to *");
                VersionReq::STAR
            }
        };

        Ok(req)
    }
}
