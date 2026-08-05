mod pack;
mod prelude;
mod resolve;

use std::collections::{BTreeMap, HashMap};

use anyhow::anyhow;
use creeper_pubgrub::{DefaultStringReporter, Reporter};
use petgraph::{algo::toposort, graph::DiGraph};
use semver::{Version, VersionReq};
use tokio::task::spawn_blocking;
use tracing::{error, info, instrument};

use crate::{
    Creeper, Id,
    index::VersionRev,
    pubgrub::{pack::Package, resolve::Resolve},
};

pub use prelude::*;

impl Creeper {
    #[instrument(skip(self, req), fields(req = req.len()))]
    pub async fn resolve(
        &self,
        req: BTreeMap<Id, VersionReq>,
    ) -> anyhow::Result<HashMap<Id, VersionRev>> {
        let resolve = Resolve::new(self.clone(), req);

        resolve.prepare().await?;

        let res = spawn_blocking(move || {
            creeper_pubgrub::resolve(&resolve, Package::Root, Version::new(0, 0, 0))
        })
        .await?;

        let sol = res.map_err(|e| match e {
            creeper_pubgrub::PubGrubError::NoSolution(derivation_tree) => {
                let mut report = DefaultStringReporter::report(&derivation_tree);

                // remove the ugly double newlines in the report
                while report.find("\n\n").is_some() {
                    report = report.replace("\n\n", "\n");
                }

                anyhow!("no solution:\n{report}")
            }
            creeper_pubgrub::PubGrubError::ErrorRetrievingDependencies {
                package,
                version,
                source,
            } => anyhow!(
                "failed to retrieve dependencies for package {package} version {version}: {source}"
            ),
            creeper_pubgrub::PubGrubError::ErrorChoosingVersion { package, source } => {
                anyhow!("failed to choose version for package {package}: {source}")
            }
            creeper_pubgrub::PubGrubError::ErrorInShouldCancel(_) => {
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
    pub async fn sort_dependency(
        &self,
        dep: HashMap<Id, VersionRev>,
    ) -> anyhow::Result<Vec<(Id, VersionRev)>> {
        let mut graph = DiGraph::<&Id, ()>::new();
        let mut id_to_node = HashMap::new();
        let mut node_to_id = HashMap::new();

        for (package, _) in &dep {
            let node = graph.add_node(package);
            id_to_node.insert(package, node);
            node_to_id.insert(node, package);
        }

        for (package, version) in &dep {
            let node = self
                .get_node(package, &version.version, version.rev)
                .await?;
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
