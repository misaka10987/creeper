use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt::{Debug, Display},
    hash::Hash,
    iter::once,
    ops::Deref,
    sync::RwLock,
};

use anyhow::anyhow;
use creeper_semver_pubgrub::SemverPubgrub;
use petgraph::{algo::toposort, graph::DiGraph};
use pubgrub::{DefaultStringReporter, Dependencies, DependencyProvider, Reporter};
use semver::{BuildMetadata, Prerelease, Version, VersionReq};
use tracing::{debug, error, info, trace, warn};

use crate::{Creeper, Id, index::VersionRev, pack::PackNode};

struct Error(anyhow::Error);

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

#[derive(Clone, PartialEq, Eq, Hash)]
enum Package {
    Normal(Id),
    Root,
    OneHot(Conflict),
    Either(BTreeMap<Id, VersionReq>),
}

impl Display for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Package::Normal(id) => write!(f, "{id}"),
            Package::Root => write!(f, "<root>"),
            Package::OneHot(clause) => write!(f, "{clause}"),
            Package::Either(btree_map) => {
                let data = btree_map
                    .iter()
                    .map(|(k, v)| format!("{k}@{v}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                write!(f, "<either: {data}>")
            }
        }
    }
}

impl Debug for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
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
        write!(f, "<onehot: {data}>")
    }
}

struct ConflictManager {
    clause: HashSet<Conflict>,
}

impl ConflictManager {
    fn new() -> Self {
        Self {
            clause: HashSet::new(),
        }
    }

    fn get_dependencies(&self, package: &Id, version: &Version) -> HashMap<Package, VersionReq> {
        let mut deps = HashMap::new();

        for clause in &self.clause {
            if let Some(version) = clause.dep_of(package, version) {
                deps.insert(
                    Package::OneHot(clause.clone()),
                    format!("={version}").parse().unwrap(),
                );
            }
        }

        deps
    }

    /// Simplify the conflict clauses logically, e.g. deduplication or removing clauses implied by others,
    /// in order to improve performance.
    // the function shall not exceed O(n^2) in time complexity
    fn simp(&mut self) {
        self.clause
            .retain(|x| !x.keys().all(|k| [Id::neoforge(), Id::fabric()].contains(k)));
        self.clause.insert(Conflict(
            [
                (Id::neoforge(), VersionReq::STAR),
                (Id::fabric(), VersionReq::STAR),
            ]
            .into_iter()
            .collect(),
        ));
        warn!("TODO: simplify conflict clauses to improve performance");
    }
}

impl Extend<Conflict> for ConflictManager {
    fn extend<T: IntoIterator<Item = Conflict>>(&mut self, iter: T) {
        for i in iter {
            self.clause.insert(i.into());
        }
    }
}

struct Resolve {
    lib: Creeper,
    root: PackNode,
    conflict: RwLock<ConflictManager>,
}

impl Resolve {
    fn new(lib: Creeper, root: PackNode) -> Self {
        Self {
            lib,
            root,
            conflict: RwLock::new(ConflictManager::new()),
        }
    }

    fn prepare(&self) -> anyhow::Result<()> {
        let reachable = self
            .lib
            .blocking_get_reachable_package(self.root.clone().neighbours())?;

        let mut clause = vec![];

        for id in reachable {
            let index = self.lib.blocking_get_index(&id)?;

            clause.extend(
                index
                    .into_iter()
                    .filter_map(|(VersionRev(v, _), node)| node.conflict_clause(id.clone(), v)),
            );
        }

        debug!("prepared {} conflict clauses", clause.len());

        let mut conflict = self.conflict.write().unwrap();

        conflict.extend(clause);

        conflict.simp();

        Ok(())
    }
}

impl DependencyProvider for Resolve {
    type P = Package;

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
        let package = match package {
            Package::Normal(id) => id,
            Package::Root => return Reverse(usize::MAX),
            Package::OneHot(btree_map) => return Reverse(btree_map.len()),
            Package::Either(btree_map) => return Reverse(btree_map.len()),
        };

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
        let available = match package {
            Package::Normal(id) => self
                .lib
                .blocking_get_index(id)?
                .into_keys()
                .map(|VersionRev(v, _)| v)
                .filter(|v| range.contains(v))
                .collect::<BTreeSet<_>>(),
            Package::Root => return Ok(Some(Version::new(0, 0, 0))),
            Package::OneHot(clause) => clause
                .versions()
                .filter(|v| range.contains(v))
                .collect::<BTreeSet<_>>(),
            Package::Either(map) => (1..=map.len())
                .map(|i| Version::new(i as u64, 0, 0))
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

    // TODO: add conflict virtual packages to dependencies
    fn get_dependencies(
        &self,
        package: &Self::P,
        version: &Self::V,
    ) -> Result<pubgrub::Dependencies<Self::P, Self::VS, Self::M>, Self::Err> {
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
            Package::OneHot(_) => return Ok(Dependencies::Available(Default::default())),
            Package::Either(map) => {
                if version.major > map.len() as u64
                    || version.minor != 0
                    || version.patch != 0
                    || version.pre != Prerelease::EMPTY
                    || version.build != BuildMetadata::EMPTY
                {
                    return Err(
                        anyhow!("invalid version {version} for virtual package {package}").into(),
                    );
                }

                let (id, req) = map.iter().nth(version.major as usize - 1).unwrap();

                let dep = once((Package::Normal(id.clone()), SemverPubgrub::from(req))).collect();

                return Ok(Dependencies::Available(dep));
            }
        };

        let index = self.lib.blocking_get_index(package)?;

        // TODO: support revision number instead of defaulting to 0
        let node = &index[&VersionRev(version.clone(), 0)];

        let conflict = self
            .conflict
            .read()
            .unwrap()
            .get_dependencies(package, version);

        let mut either_dep = vec![];

        for grp in node.either_dep.clone() {
            let clause = grp.into_iter().collect::<BTreeMap<_, _>>();
            either_dep.push((Package::Either(clause), VersionReq::STAR));
        }

        let dep = node
            .dep
            .iter()
            .map(|(k, v)| (Package::Normal(k.clone()), SemverPubgrub::from(v)))
            .chain(
                conflict
                    .into_iter()
                    .chain(either_dep)
                    .map(|(k, v)| (k, SemverPubgrub::from(&v))),
            )
            .collect();

        Ok(Dependencies::Available(dep))
    }
}

impl Creeper {
    pub fn resolve(&self, req: BTreeMap<Id, VersionReq>) -> anyhow::Result<HashMap<Id, Version>> {
        info!("resolving {} required packages", req.len());

        let resolve = Resolve::new(
            self.clone(),
            PackNode {
                dep: req,
                ..Default::default()
            },
        );

        resolve.prepare()?;

        let res = pubgrub::resolve(&resolve, Package::Root, Version::new(0, 0, 0));

        let sol = res.map_err(|e| match e {
            pubgrub::PubGrubError::NoSolution(derivation_tree) => {
                let mut report = DefaultStringReporter::report(&derivation_tree);

                // remove the ugly double newlines in the report
                while report.find("\n\n").is_some() {
                    report = report.replace("\n\n", "\n");
                }

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

        let sol = sol.into_iter();

        let all = sol.len();

        // PubGrub uses non-default hasher, convert to standard before returning
        let sol = sol
            .filter_map(|(k, v)| match k {
                Package::Normal(id) => Some((id, v)),
                _ => None,
            })
            .collect::<HashMap<_, _>>();

        let real = sol.len();

        info!(
            "resolved {all} packages, of which {real} real and {} virtual",
            all - real
        );

        Ok(sol)
    }

    /// Topologically sort the dependencies. Dependencies goes before dependents in the output.
    ///
    /// The behavior is undefined unless the input is a valid solution, i.e. dependencies of each package in the input are also present in the input.
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
            let node = self.blocking_get_node(package, version, 0)?;
            let node_package = id_to_node[package];

            for (d, _) in node.dep {
                let node_dep = id_to_node
                    .get(&d)
                    .ok_or(anyhow!("broken solution: dependency {d} not recorded"))?;

                graph.add_edge(node_package, *node_dep, ());
            }

            for (d, _) in node.either_dep.into_iter().flatten() {
                if let Some(node_dep) = id_to_node.get(&d) {
                    error!(
                        "TODO: avoid assuming {package} dependency on {d}, which may be incorrect"
                    );
                    graph.add_edge(node_package, *node_dep, ());
                }
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
