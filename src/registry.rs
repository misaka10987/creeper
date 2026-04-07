use std::{
    cmp::Reverse,
    collections::{BTreeSet, HashMap},
    fs::read_to_string,
    path::PathBuf,
};

use anyhow::{anyhow, bail};
use creeper_semver_pubgrub::SemverPubgrub;
use pubgrub::{Dependencies, DependencyProvider};
use semver::{Version, VersionReq};
use tracing::{debug, error, trace};
use url::Url;

use crate::{Id, pack::PackNode};

pub struct Registry {
    pub url: Url,
    path: PathBuf,
}

impl Registry {
    pub async fn new(url: Url) -> anyhow::Result<Self> {
        let path = url
            .to_file_path()
            .expect("TODO: only file:// URLs are supported for now");
        Ok(Self { url, path })
    }

    pub fn get(&self, package: &Id, version: &Version, rev: u32) -> anyhow::Result<PackNode> {
        let path = self
            .path
            .join(package.indexed_path())
            .join(version.to_string())
            .join(rev.to_string())
            .with_extension("toml");
        let content = read_to_string(path)?;
        let node = toml::from_str(&content)?;
        Ok(node)
    }

    pub fn get_version(&self, package: &Id) -> anyhow::Result<BTreeSet<Version>> {
        trace!("retrieving versions for package {package}");
        let path = self.path.join(package.indexed_path());
        let mut res = BTreeSet::new();
        for i in path.read_dir()? {
            let entry = i?;
            if !entry.file_type()?.is_dir() {
                bail!(
                    "invalid package registry item {}, expected a directory",
                    entry.path().display()
                );
            }
            let name = entry.file_name().into_string().map_err(|s| {
                anyhow!("invalid package registry item {s:?}, expected valid UTF-8 file name")
            })?;
            if let Ok(version) = name.parse() {
                res.insert(version);
            } else {
                bail!(
                    "invalid package registry item {}, expected a semver version",
                    entry.path().display()
                );
            }
        }
        Ok(res)
    }

    pub fn resolve(&self, req: HashMap<Id, VersionReq>) -> anyhow::Result<HashMap<Id, Version>> {
        struct Resolve<'a> {
            registry: &'a Registry,
            req: HashMap<Id, VersionReq>,
        }

        impl<'a> DependencyProvider for Resolve<'a> {
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
                self.registry
                    .prioritize(package, range, _package_conflicts_counts)
            }

            fn choose_version(
                &self,
                package: &Self::P,
                range: &Self::VS,
            ) -> Result<Option<Self::V>, Self::Err> {
                if *package == Id::root() {
                    return Ok(Some(Version::new(0, 0, 0)));
                }
                self.registry.choose_version(package, range)
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
                self.registry.get_dependencies(package, version)
            }
        }

        debug!("resolving {} required packages", req.len());

        let resolve = Resolve {
            registry: self,
            req,
        };
        let sol = pubgrub::resolve(&resolve, Id::root(), Version::new(0, 0, 0));
        // PubGrub's error holds internal references, thus we convert that to string to avoid lifetime issues
        let sol = sol.map_err(|e| anyhow!("resolution error: {e}"))?;
        // PubGrub uses non-default hasher, convert to standard before returning
        let sol = sol.into_iter().collect();
        Ok(sol)
    }
}

impl DependencyProvider for Registry {
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
        trace!("determining priority for package {package}");
        let candidates = self.get_version(package).unwrap_or_else(|e| {
            error!("failed to prioritize package {package}: {e}");
            error!("package resolution will continue with no available versions for this package");
            BTreeSet::new()
        });
        let available = candidates.iter().filter(|v| range.contains(v)).count();
        Reverse(available)
    }

    fn choose_version(
        &self,
        package: &Self::P,
        range: &Self::VS,
    ) -> Result<Option<Self::V>, Self::Err> {
        let candidates = self.get_version(package)?;
        let available = candidates
            .into_iter()
            .filter(|v| range.contains(v))
            .collect::<BTreeSet<_>>();
        let highest = available.last();
        trace!("selected version {highest:?} for package {package}");
        Ok(highest.cloned())
    }

    fn get_dependencies(
        &self,
        package: &Self::P,
        version: &Self::V,
    ) -> Result<pubgrub::Dependencies<Self::P, Self::VS, Self::M>, Self::Err> {
        // TODO: support revision number instead of defaulting to 0
        let node = self.get(package, version, 0)?;
        let dep = node
            .dep
            .into_iter()
            .map(|(k, v)| (k, SemverPubgrub::from(&v)))
            .collect();
        Ok(Dependencies::Available(dep))
    }
}
