use std::{collections::BTreeSet, fs::read_to_string, path::PathBuf};

use anyhow::{anyhow, bail};
use base64::{Engine, prelude::BASE64_URL_SAFE};
use semver::Version;
use tokio::{fs::try_exists, process::Command};
use tracing::{debug, trace};
use url::Url;

use crate::{Creeper, Id, pack::PackNode, path::creeper_cache_dir};

pub struct Registry {
    pub url: Url,
    path: PathBuf,
}

impl Registry {
    pub fn cache_path(url: &Url) -> anyhow::Result<PathBuf> {
        let path = creeper_cache_dir()?
            .join("registry")
            // URL contains invalid characters for filesystem
            .join(BASE64_URL_SAFE.encode(url.as_str()));
        Ok(path)
    }

    pub fn new(url: Url) -> anyhow::Result<Self> {
        let path = match url.scheme() {
            "file" => {
                let path = url
                    .to_file_path()
                    .map_err(|_| anyhow!("invalid file URL: {url}"))?;
                debug!("using local registry at {}", path.display());
                path
            }
            "git+https" => Self::cache_path(&url)?,
            s => bail!("unsupported registry URL scheme: {s}"),
        };
        Ok(Self { path, url })
    }

    async fn update(&self) -> anyhow::Result<()> {
        match self.url.scheme() {
            "file" => return Ok(()),
            "git+https" => {
                if !try_exists(&self.path).await? {
                    let url = self
                        .url
                        .as_str()
                        .strip_prefix("git+")
                        .unwrap()
                        .parse::<Url>()?;
                    debug!("downloading registry: {url}");
                    let status = Command::new("git")
                        .arg("clone")
                        .arg("--depth")
                        .arg("1")
                        .arg(url.as_str())
                        .arg(&self.path)
                        .spawn()?
                        .wait()
                        .await?;
                    if !status.success() {
                        bail!("unable to download registry, command run failed");
                    }
                    return Ok(());
                }
                debug!("updating registry {}", self.url);
                let status = Command::new("git")
                    .current_dir(&self.path)
                    .arg("pull")
                    .spawn()?
                    .wait()
                    .await?;
                if !status.success() {
                    bail!("unable to update registry, command run failed");
                }
                Ok(())
            }
            s => panic!("unsupported registry URL scheme: {s}"),
        }
    }

    pub fn get(&self, package: &Id, version: &Version, _rev: u32) -> anyhow::Result<PackNode> {
        let path = self
            .path
            .join(package.indexed_path())
            .join(format!("{version}.toml"));
        trace!("loading {package} {version} from {}", path.display());
        let content = read_to_string(path)?;
        let node = toml::from_str(&content)?;
        Ok(node)
    }

    pub fn get_version(&self, package: &Id) -> anyhow::Result<BTreeSet<Version>> {
        trace!("retrieving versions for {package}");
        let path = self.path.join(package.indexed_path());
        let mut res = BTreeSet::new();
        for i in path.read_dir()? {
            let entry = i?;
            let path = entry.path();
            if !entry.file_type()?.is_file() || path.extension().is_none_or(|s| s != "toml") {
                bail!(
                    "invalid package registry item {}, expected TOML file",
                    path.display()
                );
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or(anyhow!("failed to retrieve version from filename"))?;

            res.insert(name.parse()?);
        }
        trace!("found {} version(s) for {}", res.len(), package);
        Ok(res)
    }
}

impl Creeper {
    pub async fn update_registry(&self) -> anyhow::Result<()> {
        self.registry.update().await
    }
}
