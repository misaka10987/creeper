use std::{cmp::Reverse, iter::Rev, path::PathBuf};

use anyhow::{anyhow, bail};
use pubgrub::{Dependencies, DependencyProvider, Ranges};
use semver::Version;
use stop::fatal;
use tokio::fs::read_to_string;
use url::Url;
use walkdir::WalkDir;

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

    pub async fn get(&self, package: &Id) -> anyhow::Result<Vec<PackNode>> {
        let path = self.path.join(package.indexed_path());
        let mut res = vec![];
        for i in WalkDir::new(path) {
            let entry = i?;
            let content = read_to_string(entry.path()).await?;
            let node = toml::from_str(&content)?;
            res.push(node);
        }
        Ok(res)
    }

    pub fn blocking_get(&self, package: &Id) -> anyhow::Result<Vec<PackNode>> {
        let path = self.path.join(package.indexed_path());
        let mut res = vec![];
        for i in WalkDir::new(path) {
            let entry = i?;
            let content = std::fs::read_to_string(entry.path())?;
            let node = toml::from_str(&content)?;
            res.push(node);
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
        let available = self
            .blocking_get(package)
            .unwrap_or_else(fatal!())
            .into_iter()
            .filter(|v| range.contains(&v.version));
        Reverse(available.count())
    }

    fn choose_version(
        &self,
        package: &Self::P,
        range: &Self::VS,
    ) -> Result<Option<Self::V>, Self::Err> {
        let mut available = self
            .blocking_get(package)
            .unwrap_or_else(fatal!())
            .into_iter()
            .filter(|v| range.contains(&v.version))
            .collect::<Vec<_>>();
        available.sort_by(|a, b| a.version.cmp(&b.version));
        Ok(available.last().map(|v| v.version.clone()))
    }

    fn get_dependencies(
        &self,
        package: &Self::P,
        version: &Self::V,
    ) -> Result<pubgrub::Dependencies<Self::P, Self::VS, Self::M>, Self::Err> {
        let chosen = self
            .blocking_get(package)
            .unwrap_or_else(fatal!())
            .into_iter()
            .find(|v| v.version == *version)
            .expect("version chosen by pubgrub is missing");
        Ok(Dependencies::Available(
            chosen
                .deps
                .into_iter()
                .map(|(k, v)| (k, ranges_for(v).unwrap_or_else(fatal!())))
                .collect(),
        ))
    }
}
