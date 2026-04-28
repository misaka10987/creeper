use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::{Debug, Display},
};

use anyhow::anyhow;
use creeper_semver_pubgrub::SemverPubgrub;
use petgraph::{algo::toposort, graph::DiGraph};
use pubgrub::{DefaultStringReporter, Dependencies, DependencyProvider, Reporter};
use semver::{Version, VersionReq};
use tracing::{debug, error, trace};

use crate::{Creeper, Id, index::VersionRev};

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

impl Creeper {
    pub fn resolve(&self, req: HashMap<Id, VersionReq>) -> anyhow::Result<HashMap<Id, Version>> {
        struct Resolve {
            lib: Creeper,
            req: HashMap<Id, VersionReq>,
        }

        impl DependencyProvider for Resolve {
            type P = Id;

            type V = Version;

            type VS = SemverPubgrub<Version>;

            type Priority = Reverse<usize>;

            type M = String;

            type Err = crate::pubgrub::Error;

            fn prioritize(
                &self,
                package: &Self::P,
                range: &Self::VS,
                // TODO(konsti): Are we always refreshing the priorities when `PackageResolutionStatistics`
                // changed for a package?
                _package_conflicts_counts: &pubgrub::PackageResolutionStatistics,
            ) -> Self::Priority {
                if *package == Id::root() {
                    return Reverse(usize::MAX);
                }

                trace!("determining priority for {package}");

                let index = self.lib.blocking_get_index(package).unwrap_or_else(|e| {
                    error!("failed to prioritize package {package}: {e}");
                    error!("package resolution will continue with no available versions for this package");
                    BTreeMap::new()
                });

                let available = index
                    .keys()
                    .map(|VersionRev(v, _)| v)
                    .filter(|v| range.contains(v))
                    .count();

                trace!("priority for {package} is {available} (smaller is higher)");
                Reverse(available)
            }

            fn choose_version(
                &self,
                package: &Self::P,
                range: &Self::VS,
            ) -> Result<Option<Self::V>, Self::Err> {
                if *package == Id::root() {
                    return Ok(Some(Version::new(0, 0, 0)));
                }

                let index = self.lib.blocking_get_index(package)?;

                let available = index
                    .keys()
                    .map(|VersionRev(v, _)| v)
                    .filter(|v| range.contains(v))
                    .collect::<BTreeSet<_>>();

                let highest = available.last();

                if let Some(version) = highest {
                    trace!("selected {package} {version}",);
                } else {
                    trace!("no available version for {package} in {range}");
                };

                Ok(highest.cloned().cloned())
            }

            fn get_dependencies(
                &self,
                package: &Self::P,
                version: &Self::V,
            ) -> Result<pubgrub::Dependencies<Self::P, Self::VS, Self::M>, Self::Err> {
                if *package == Id::root() {
                    return Ok(Dependencies::Available(
                        self.req
                            .iter()
                            .map(|(k, v)| (k.clone(), SemverPubgrub::from(v)))
                            .collect(),
                    ));
                }

                let index = self.lib.blocking_get_index(package)?;

                // TODO: support revision number instead of defaulting to 0
                let node = &index[&VersionRev(version.clone(), 0)];

                let dep = node
                    .dep
                    .iter()
                    .map(|(k, v)| (k.clone(), SemverPubgrub::from(v)))
                    .collect();

                Ok(Dependencies::Available(dep))
            }
        }

        debug!("resolving {} required packages", req.len());

        let resolve = Resolve {
            lib: self.clone(),
            req,
        };

        let res = pubgrub::resolve(&resolve, Id::root(), Version::new(0, 0, 0));

        let mut sol = res.map_err(|e| match e {
            pubgrub::PubGrubError::NoSolution(derivation_tree) => {
                let report = DefaultStringReporter::report(&derivation_tree);
                anyhow!("no solution:\n{report}")
            }
            pubgrub::PubGrubError::ErrorRetrievingDependencies {
                package,
                version,
                source,
            } => anyhow!(
                "failed to retrieve dependencies for package {package} version {version}: {source}"
            ),
            pubgrub::PubGrubError::ErrorChoosingVersion { package, source } => {
                anyhow!("failed to choose version for package {package}: {source}")
            }
            pubgrub::PubGrubError::ErrorInShouldCancel(_) => {
                anyhow!("package resolution cancelled")
            }
        })?;

        sol.remove(&Id::root());

        // PubGrub uses non-default hasher, convert to standard before returning
        let sol = sol.into_iter().collect();

        Ok(sol)
    }

    pub fn sort_dependency(&self, dep: HashMap<Id, Version>) -> anyhow::Result<Vec<(Id, Version)>> {
        let mut graph = DiGraph::<&Id, ()>::new();
        let mut id_to_node = HashMap::new();
        let mut node_to_id = HashMap::new();

        for (package, _) in &dep {
            let node = graph.add_node(package);
            id_to_node.insert(package, node);
            node_to_id.insert(node, package);
        }

        for (package, version) in &dep {
            if !package.is_regular() {
                todo!()
            }

            let dep = self.registry.get_node(package, version, 0)?.dep;

            for (d, _) in dep {
                let node_package = id_to_node[package];
                let node_dep = id_to_node
                    .get(&d)
                    .ok_or(anyhow!("broken solution: dependency {d} not recorded"))?;
                graph.add_edge(node_package, *node_dep, ());
            }
        }

        let order = toposort(&graph, None).map_err(|e| {
            let package = graph[e.node_id()];
            error!("cycle detected around package {package}");
            anyhow!("cycle in dependency DAG")
        })?;

        let order = order
            .into_iter()
            .rev()
            .map(|node| (node_to_id[&node].clone(), dep[node_to_id[&node]].clone()))
            .collect();

        Ok(order)
    }
}
