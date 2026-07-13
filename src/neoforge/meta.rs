use std::collections::HashMap;
use std::path::PathBuf;

use parse_display::{Display, FromStr};
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use serde_with::{DeserializeFromStr, NoneAsEmptyString, SerializeDisplay, serde_as};
use url::Url;

use crate::MavenVersionRange;

#[serde_inline_default]
#[serde_as]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NeoforgeMods {
    #[serde_inline_default("javafml".into())]
    pub mod_loader: String,

    // TODO: serde with maven version range
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<MavenVersionRange>,

    /// This field should be a valid SPDX license expression,
    /// but since many mods like Sodium violate this rule,
    /// the field is typed as a `String`.
    // #[serde_as(as = "DisplayFromStr")]
    // pub license: Expression,
    pub license: String,

    #[serde_inline_default(false)]
    pub show_as_resource_pack: bool,

    #[serde_inline_default(false)]
    pub show_as_data_pack: bool,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<String>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub properties: HashMap<String, String>,

    #[serde(rename = "issueTrackerURL", skip_serializing_if = "Option::is_none")]
    pub issue_tracker_url: Option<Url>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mods: Vec<Mod>,

    /// NeoForge does not document the type of a property, so we use `serde_json::Value` to represent it.
    #[serde(
        rename = "modproperties",
        default,
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub mod_properties: HashMap<String, HashMap<String, serde_json::Value>>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub access_transformers: Vec<AccessTransformer>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mixins: Vec<Mixin>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub dependencies: HashMap<String, Vec<Dependency>>,
}

#[serde_inline_default]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Mod {
    pub mod_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    #[serde_inline_default("1".into())]
    pub version: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    #[serde_inline_default(r#"'''MISSING DESCRIPTION '''"#.into())]
    pub description: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_file: Option<PathBuf>,

    #[serde_inline_default(true)]
    pub logo_blur: bool,

    #[serde(rename = "updateJSONURL", skip_serializing_if = "Option::is_none")]
    pub update_json_url: Option<Url>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub mod_url: Option<Url>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub authors: Option<String>,

    #[serde(rename = "displayURL", skip_serializing_if = "Option::is_none")]
    pub display_url: Option<Url>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub enum_extensions: Option<PathBuf>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature_flags: Option<PathBuf>,

    /// The field is not NeoForge standard but used by Sodium.
    /// It is here to avoid error.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub _provides: Vec<String>,

    /// The field is not NeoForge standard but used by Iris.
    /// It is here to avoid error.
    #[serde(rename = "sodium:options", default, skip_serializing)]
    pub _sodium_options: serde_json::Value,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AccessTransformer {
    pub file: PathBuf,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Mixin {
    pub config: PathBuf,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_mods: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub behavior_version: Option<String>,
}

#[serde_inline_default]
#[serde_as]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Dependency {
    pub mod_id: String,

    #[serde_inline_default(DependencyType::Required)]
    #[serde(rename = "type")]
    pub dependency_type: DependencyType,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_range: Option<MavenVersionRange>,

    #[serde_inline_default(Ordering::None)]
    pub ordering: Ordering,

    #[serde_inline_default(Side::Both)]
    pub side: Side,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub refferal_url: Option<Url>,
}

#[derive(Clone, PartialEq, Eq, Display, FromStr, SerializeDisplay, DeserializeFromStr)]
#[display(style = "camelCase")]
pub enum DependencyType {
    Required,
    Optional,
    Incompatible,
    Discouraged,
}

#[derive(Clone, PartialEq, Eq, Display, FromStr, SerializeDisplay, DeserializeFromStr)]
#[display(style = "SNAKE_CASE")]
pub enum Ordering {
    Before,
    After,
    None,
}

#[derive(Clone, PartialEq, Eq, Display, FromStr, SerializeDisplay, DeserializeFromStr)]
#[display(style = "SNAKE_CASE")]
pub enum Side {
    Both,
    Client,
    Server,
}
