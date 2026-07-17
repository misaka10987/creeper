use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::RwLock,
};

use anyhow::bail;
use base64::{Engine, prelude::BASE64_URL_SAFE};
use reqwest::Client;
use semver::Version;
use tokio::{
    fs::{File, create_dir_all, read_to_string, try_exists},
    io::AsyncWriteExt,
    process::Command,
};
use tracing::{debug, info};
use url::Url;

use crate::{
    Creeper, Id, Package,
    index::{Index, IndexLine, VersionRev},
    path::creeper_cache_dir,
    tool::BuildIndex,
};

pub struct Registry {
    pub url: Url,
    http: Client,
    cache: RwLock<HashMap<Id, BTreeMap<VersionRev, Package>>>,
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
        Ok(Self {
            url,
            http,
            cache: RwLock::new(HashMap::new()),
        })
    }

    async fn index_url(&self) -> anyhow::Result<Url> {
        if self.url.scheme() == "file" {
            let path = self.index_cache_path()?;
            let url = format!("file://{}", path.display()).parse()?;
            return Ok(url);
        }

        let url_cache = self.cache_path()?.join("package-index.url");

        if try_exists(&url_cache).await? {
            let url = read_to_string(&url_cache).await?;
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

    pub fn blocking_get_index(&self, package: &Id) -> anyhow::Result<Index> {
        let path = self
            .index_cache_path()?
            .join("index")
            .join(package.indexed_path())
            .with_added_extension("jsonl");

        if !path.exists() {
            bail!("package {package} does not exist or missing from cache");
        }

        let pack = IndexLine::blocking_read(path)?;

        Ok(pack)
    }

    pub async fn get_index(&self, package: &Id) -> anyhow::Result<Index> {
        let path = self
            .index_cache_path()?
            .join("index")
            .join(package.indexed_path())
            .with_added_extension("jsonl");

        if !try_exists(&path).await? {
            bail!("package {package} does not exist or missing from cache");
        }

        let pack = IndexLine::read(path).await?;

        Ok(pack)
    }

    pub async fn get(&self, id: &Id, version: &Version, rev: u32) -> anyhow::Result<Package> {
        if let Some(pack) = self.cache.read().unwrap().get(id) {
            if let Some(pack) = pack.get(&VersionRev::with_rev(version.clone(), rev)) {
                return Ok(pack.clone());
            }
        }

        if self.url.scheme() == "file" {
            let path = self.url.path();

            let path = PathBuf::from(path)
                .join(id.indexed_path())
                .join(version.to_string())
                .join(rev.to_string())
                .with_added_extension("toml");

            if !try_exists(&path).await? {
                bail!("{id}@{version} rev {rev} does not exist");
            }

            let toml = read_to_string(&path).await?;

            let pack = toml::from_str::<Package>(&toml)?;

            self.cache
                .write()
                .unwrap()
                .entry(id.clone())
                .or_default()
                .insert(VersionRev::with_rev(version.clone(), rev), pack.clone());

            return Ok(pack);
        }

        let url = self
            .url
            .join("package/")?
            .join(&format!(
                "{}/",
                id.indexed_path().as_ref().to_str().unwrap()
            ))?
            .join(&format!("{version}/"))?
            .join(&format!("{rev}.json"))?;

        let req = self.http.get(url).build()?;
        let res = self.http.execute(req).await?;

        let pack = res.json::<Package>().await?;

        self.cache
            .write()
            .unwrap()
            .entry(id.clone())
            .or_default()
            .insert(VersionRev::with_rev(version.clone(), rev), pack.clone());
        Ok(pack)
    }
}

impl Creeper {
    pub async fn update_registry(&self) -> anyhow::Result<()> {
        if self.args.offline {
            info!("skipping registry update because offline mode enabled");
            return Ok(());
        }

        info!("updating registry {}", self.registry.url);

        let cache = self.registry.index_cache_path()?;
        let url = self.registry.index_url().await?;

        match url.scheme() {
            "file" => {
                let cmd = BuildIndex {
                    input: self.registry.url.path().into(),
                    output: Some(cache.join("index")),
                };

                self.execute(cmd).await
            }
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

    pub async fn query_registry(
        &self,
        package: &Id,
        version: &Version,
        rev: u32,
    ) -> anyhow::Result<Package> {
        self.registry.get(package, version, rev).await
    }
}
