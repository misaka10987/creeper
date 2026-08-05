use std::{
    collections::{BTreeMap, HashSet},
    fmt::{Debug, Display},
    hash::Hash,
    ops::Deref,
};

use anyhow::bail;
use semver::{BuildMetadata, Prerelease, Version, VersionReq};
use tracing::warn;

use crate::{
    Id,
    builtin::{fabric_id, neoforge_id, neoforge_server_id, vanilla_id, vanilla_server_id},
};

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Package {
    Normal(Id),
    Root,
    Either(Either),
}

impl Display for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Package::Normal(id) => write!(f, "{id}"),
            Package::Root => write!(f, "<root>"),
            // Package::OneHot(clause) => write!(f, "{clause}"),
            Package::Either(clause) => write!(f, "{clause}"),
        }
    }
}

impl Debug for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

/// An "either" clause.
/// Denotes that any of the requirements will satisfy the clause, but not necessarily all of them.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Either(pub BTreeMap<Id, VersionReq>);

impl Deref for Either {
    type Target = BTreeMap<Id, VersionReq>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Either {
    pub fn versions(&self) -> impl Iterator<Item = Version> {
        (1..=self.len()).map(|i| Version::new(i as u64, 0, 0))
    }

    pub fn select(&self, version: &Version) -> anyhow::Result<(&Id, &VersionReq)> {
        if version.major == 0
            || version.major > self.len() as u64
            || version.minor != 0
            || version.patch != 0
            || version.pre != Prerelease::EMPTY
            || version.build != BuildMetadata::EMPTY
        {
            bail!("invalid version {version} for either clause {self}");
        }

        let (id, req) = self.iter().nth(version.major as usize - 1).unwrap();

        Ok((id, req))
    }
}

impl Display for Either {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let data = self
            .iter()
            .map(|(k, v)| format!("{k}@{v}"))
            .collect::<Vec<_>>()
            .join(" ");
        write!(f, "<either: {data}>")
    }
}

/// A conflict, or "onehot" clause.
/// Denotes that at most one of the requirements can be satisfied at the same time.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Conflict(pub BTreeMap<Id, VersionReq>);

impl Conflict {
    pub fn versions(&self) -> impl Iterator<Item = Version> {
        (1..=self.len()).map(|i| Version::new(i as u64, 0, 0))
    }

    /// If the clause is depended by the given package, returns the specific version being depended on.
    /// Otherwise, return `None`.
    pub fn dep_of(&self, package: &Id, version: &Version) -> Option<Version> {
        self.iter()
            .position(|(id, req)| id == package && req.matches(version))
            .map(|i| Version::new(i as u64 + 1, 0, 0))
    }
}

impl Deref for Conflict {
    type Target = BTreeMap<Id, VersionReq>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<BTreeMap<Id, VersionReq>> for Conflict {
    fn from(value: BTreeMap<Id, VersionReq>) -> Self {
        Self(value)
    }
}

impl From<Conflict> for BTreeMap<Id, VersionReq> {
    fn from(value: Conflict) -> Self {
        value.0
    }
}

impl Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let data = self
            .iter()
            .map(|(k, v)| format!("{k}@{v}"))
            .collect::<Vec<_>>()
            .join(" ");
        write!(f, "<conflict: {data}>")
    }
}

pub struct ConflictManager {
    clause: HashSet<Conflict>,
}

impl ConflictManager {
    pub fn new() -> Self {
        Self {
            clause: HashSet::new(),
        }
    }

    /// Simplify the conflict clauses logically, e.g. deduplication or removing clauses implied by others,
    /// in order to improve performance.
    // the function shall not exceed O(n^2) in time complexity
    pub fn simp(&mut self) {
        self.clause.retain(|x| {
            !x.keys().all(|k| {
                [neoforge_id(), fabric_id()].contains(k)
                    || [
                        vanilla_id(),
                        vanilla_server_id(),
                        neoforge_server_id(),
                        "server-provider".parse().unwrap(),
                    ]
                    .contains(k)
            })
        });

        let conflict = [
            [
                (neoforge_id(), VersionReq::STAR),
                (fabric_id(), VersionReq::STAR),
            ]
            .into(),
            [
                (vanilla_id(), VersionReq::STAR),
                (vanilla_server_id(), VersionReq::STAR),
                (neoforge_server_id(), VersionReq::STAR),
                ("server-provider".parse().unwrap(), VersionReq::STAR),
            ]
            .into(),
        ];

        self.clause.extend(conflict.into_iter().map(Conflict));

        warn!("TODO: simplify conflict clauses to improve performance");
    }

    pub fn as_clauses(&self) -> &HashSet<Conflict> {
        &self.clause
    }
}

impl Extend<Conflict> for ConflictManager {
    fn extend<T: IntoIterator<Item = Conflict>>(&mut self, iter: T) {
        for i in iter {
            self.clause.insert(i.into());
        }
    }
}
