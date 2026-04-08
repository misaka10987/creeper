use std::{collections::BTreeSet, fs::read_to_string, path::PathBuf};

use anyhow::{anyhow, bail};
use semver::Version;
use tracing::trace;
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
}
