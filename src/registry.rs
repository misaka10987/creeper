use std::{cmp::Reverse, collections::BTreeSet, fs::read_to_string, path::PathBuf};

use anyhow::{anyhow, bail};
use pubgrub::{Dependencies, DependencyProvider, Ranges};
use semver::Version;
use stop::fatal;
use tracing::error;
use url::Url;

use crate::{Id, pack::PackNode, pubgrub::ranges_for};

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
}

impl DependencyProvider for Registry {
    type P = Id;

    type V = Version;

    type VS = Ranges<Version>;

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
            .map(|(k, v)| (k, ranges_for(v).unwrap_or_else(fatal!())))
            .collect();
        Ok(Dependencies::Available(dep))
    }
}
