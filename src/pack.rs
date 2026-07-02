use std::{
    collections::{BTreeMap, HashSet},
    iter::once,
};

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use serde_with::{DisplayFromStr, serde_as};
use spdx::Expression;

use crate::{Id, Install, pubgrub::Conflict};

/// The package node in the dependency graph, containing only metadata needed for dependency resolution.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PackNode {
    /// Dependencies.
    #[serde(
        default,
        rename = "dependencies",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub dep: BTreeMap<Id, VersionReq>,

    #[serde(
        default,
        rename = "conflicts",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub conflict: BTreeMap<Id, VersionReq>,

    #[serde(
        default,
        rename = "either-dependency",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub either_dep: Vec<BTreeMap<Id, VersionReq>>,
}

impl PackNode {
    pub fn neighbours(self) -> HashSet<Id> {
        self.dep
            .into_keys()
            .chain(self.conflict.into_keys())
            .chain(self.either_dep.into_iter().flat_map(|grp| grp.into_keys()))
            .collect()
    }

    pub fn conflict_clause(self, id: Id, version: Version) -> Option<Conflict> {
        if self.conflict.is_empty() {
            return None;
        }

        let map = self
            .conflict
            .into_iter()
            .chain(once((id.clone(), format!("={version}").parse().unwrap())))
            .collect();

        Some(Conflict(map))
    }
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
    #[serde(default)]
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
