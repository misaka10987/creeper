use std::{collections::BTreeSet, fs::read_to_string, path::PathBuf};

use anyhow::{anyhow, bail};
use base64::{Engine, prelude::BASE64_URL_SAFE};
use reqwest::Client;
use semver::Version;
use tokio::{
    fs::{File, create_dir_all, read_to_string as async_read_to_string, try_exists},
    io::AsyncWriteExt,
    process::Command,
};
use tracing::{debug, trace};
use url::Url;

use crate::{Creeper, Id, pack::PackNode, path::creeper_cache_dir};

pub struct Registry {
    pub url: Url,
    http: Client,
}

impl Registry {
    pub fn cache_path(&self) -> anyhow::Result<PathBuf> {
        let path = creeper_cache_dir()?
            .join("registry")
            // URL contains invalid characters for filesystem
            .join(BASE64_URL_SAFE.encode(self.url.as_str()));
        Ok(path)
    }

    pub fn index_cache_path(&self) -> anyhow::Result<PathBuf> {
        Ok(self.cache_path()?.join("package-index"))
    }

    pub fn new(url: Url, http: Client) -> anyhow::Result<Self> {
        match url.scheme() {
            "file" => debug!("using local registry at {url}"),
            "https" => debug!("using remote registry at {url}"),
            s => bail!("unsupported registry URL scheme: {s}"),
        }
        Ok(Self { url, http })
    }

    async fn index_url(&self) -> anyhow::Result<Url> {
        let url_cache = self.cache_path()?.join("package-index.url");

        if try_exists(&url_cache).await? {
            let url = async_read_to_string(&url_cache).await?;
            let url = url.trim().parse()?;
            debug!("using cached index URL: {url}");
            return Ok(url);
        }

        let url_def = self.url.join("package-index.url")?;

        let req = self.http.get(url_def).build()?;
        let res = self.http.execute(req).await?;

        let url = res.text().await?;
        let url: Url = url.trim().parse()?;

        create_dir_all(url_cache.parent().unwrap()).await?;
        let mut file = File::create(&url_cache).await?;
        file.write_all(url.as_str().as_bytes()).await?;

        Ok(url)
    }

    async fn update(&self) -> anyhow::Result<()> {
        let cache = self.index_cache_path()?;
        let url = self.index_url().await?;
        match url.scheme() {
            "file" => return Ok(()),
            "git+https" => {
                if !try_exists(&cache).await? {
                    let url = url.as_str().strip_prefix("git+").unwrap().parse::<Url>()?;
                    debug!("downloading registry: {url}");
                    let status = Command::new("git")
                        .arg("clone")
                        .arg("--depth")
                        .arg("1")
                        .arg(url.as_str())
                        .arg(&cache)
                        .spawn()?
                        .wait()
                        .await?;
                    if !status.success() {
                        bail!("unable to download registry, command run failed");
                    }
                    return Ok(());
                }
                debug!("updating registry {}", url);
                let status = Command::new("git")
                    .current_dir(&cache)
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
            .index_cache_path()?
            .join(package.indexed_path())
            .join(format!("{version}.toml"));
        trace!("loading {package} {version} from {}", path.display());
        let content = read_to_string(path)?;
        let node = toml::from_str(&content)?;
        Ok(node)
    }

    pub fn get_version(&self, package: &Id) -> anyhow::Result<BTreeSet<Version>> {
        trace!("retrieving versions for {package}");
        let path = self.index_cache_path()?.join(package.indexed_path());
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
