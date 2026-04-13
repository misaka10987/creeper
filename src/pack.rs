use std::collections::HashMap;

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use serde_with::{DisplayFromStr, serde_as};
use spdx::Expression;

use crate::{Id, Install};

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
