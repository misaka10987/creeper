use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::{Debug, Display},
    iter::once,
    sync::RwLock,
};

use anyhow::anyhow;
use creeper_pubgrub::{Dependencies, DependencyProvider};
use creeper_semver_pubgrub::{SemverPubgrub, VersionLike};
use itertools::Itertools;
use semver::{Version, VersionReq};
use tracing::{debug, error, trace};

use crate::{
    Creeper, Id,
    index::VersionRev,
    pack::PackNode,
    pubgrub::pack::{ConflictManager, Either, Package},
};

pub struct Resolve {
    lib: Creeper,
    root: PackNode,
    conflict: RwLock<ConflictManager>,
}

impl Resolve {
    pub fn new(lib: Creeper, req: BTreeMap<Id, VersionReq>) -> Self {
        let root = PackNode {
            dep: req,
            ..Default::default()
        };

        Self {
            lib,
            root,
            conflict: RwLock::new(ConflictManager::new()),
        }
    }

    pub async fn prepare(&self) -> anyhow::Result<()> {
        let reachable = self
            .lib
            .get_reachable_package(self.root.clone().neighbours())
            .await?;

        let mut clause = vec![];

        for id in reachable {
            let index = self.lib.get_index(&id).await?;

            clause.extend(
                index
                    .into_iter()
                    .filter_map(|(version, node)| node.conflict_clause(id.clone(), version.into())),
            );
        }

        debug!("prepared {} conflict clauses", clause.len());

        let mut conflict = self.conflict.write().unwrap();

        conflict.extend(clause);

        conflict.simp();

        Ok(())
    }
}

impl VersionLike for VersionRev {
    fn major(&self) -> u64 {
        self.version.major
    }

    fn minor(&self) -> u64 {
        self.version.minor
    }

    fn patch(&self) -> u64 {
        self.version.patch
    }

    fn pre(&self) -> &str {
        &self.version.pre
    }
}

impl DependencyProvider for Resolve {
    type P = Package;

    type V = VersionRev;

    type VS = SemverPubgrub<VersionRev>;

    type Priority = Reverse<usize>;

    type M = String;

    type Err = Error;

    fn prioritize(
        &self,
        package: &Self::P,
        range: &Self::VS,
        // TODO(konsti): Are we always refreshing the priorities when `PackageResolutionStatistics`
        // changed for a package?
        _package_conflicts_counts: &creeper_pubgrub::PackageResolutionStatistics,
    ) -> Self::Priority {
        let package = match package {
            Package::Normal(id) => id,
            Package::Root => return Reverse(usize::MAX),
            Package::Either(btree_map) => return Reverse(btree_map.len()),
        };

        trace!("determining priority for {package}");

        let index = self.lib.blocking_get_index(package).unwrap_or_else(|e| {
            error!("failed to prioritize package {package}: {e}");
            error!("package resolution will continue with no available versions for this package");
            BTreeMap::new()
        });

        let available = index.keys().filter(|v| range.contains(v)).count();

        trace!("priority for {package} is {available} (smaller is higher)");
        Reverse(available)
    }

    fn choose_version(
        &self,
        package: &Self::P,
        range: &Self::VS,
    ) -> Result<Option<Self::V>, Self::Err> {
        let available = match package {
            Package::Normal(id) => self
                .lib
                .blocking_get_index(id)?
                .into_keys()
                .filter(|v| range.contains(v))
                .collect::<BTreeSet<_>>(),
            Package::Root => return Ok(Some(Version::new(0, 0, 0).into())),
            Package::Either(clause) => clause
                .versions()
                .map(VersionRev::new)
                .filter(|v| range.contains(v))
                .collect::<BTreeSet<_>>(),
        };

        let highest = available.last();

        if let Some(version) = highest {
            trace!("selected {package} {version}",);
        } else {
            trace!("no available version for {package} in {range}");
        };

        Ok(highest.cloned())
    }

    fn get_dependencies(
        &self,
        package: &Self::P,
        version: &Self::V,
    ) -> Result<creeper_pubgrub::Dependencies<Self::P, Self::VS, Self::M>, Self::Err> {
        let package = match package {
            Package::Normal(id) => id,
            Package::Root => {
                return Ok(Dependencies::Available(
                    self.root
                        .dep
                        .iter()
                        .map(|(k, v)| (Package::Normal(k.clone()), SemverPubgrub::from(v)))
                        .collect(),
                ));
            }
            Package::Either(clause) => {
                if version.rev != 0 {
                    return Err(
                        anyhow!("package {package} does not support revision number").into(),
                    );
                }

                let (id, req) = clause.select(&version.version)?;

                let dep = once((Package::Normal(id.clone()), SemverPubgrub::from(req))).collect();

                return Ok(Dependencies::Available(dep));
            }
        };

        let index = self.lib.blocking_get_index(package)?;

        let node = &index[version];

        let either = node
            .either_dep
            .clone()
            .into_iter()
            .map(|x| (Package::Either(Either(x)), VersionReq::STAR));

        let dep = node
            .dep
            .iter()
            .map(|(k, v)| (Package::Normal(k.clone()), SemverPubgrub::from(v)))
            .chain(
                // conflict
                //     .into_iter()
                // .chain(either)
                either.map(|(k, v)| (k, SemverPubgrub::from(&v))),
            )
            .collect();

        Ok(Dependencies::Available(dep))
    }

    fn init_conflict(&self) -> Vec<HashMap<Self::P, Self::VS>> {
        let read = self.conflict.read().unwrap();

        let all = read
            .as_clauses()
            .iter()
            .map(|clause| {
                clause.iter().combinations(2).map(|pair| {
                    pair.iter()
                        .map(|(k, v)| (Package::Normal((*k).clone()), SemverPubgrub::from(*v)))
                        .collect::<HashMap<_, _>>()
                })
            })
            .flatten()
            .collect::<Vec<_>>();

        debug!(
            "converted {} conflict clauses into {} exclusive pairs",
            read.as_clauses().len(),
            all.len()
        );

        all
    }
}

pub struct Error(anyhow::Error);

impl From<anyhow::Error> for Error {
    fn from(value: anyhow::Error) -> Self {
        Self(value)
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}
