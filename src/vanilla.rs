use std::{collections::HashMap, sync::OnceLock};

use crate::{
    Checksum, Creeper,
    http::HttpRequest,
    mc::{check_class, check_os},
    pack::FileMap,
    storage::StorageManage,
};

use anyhow::anyhow;
use mc_launchermeta::{
    VERSION_MANIFEST_URL,
    version::{Download, Version as McVersion},
    version_manifest::Manifest,
};

use semver::Version;

use tokio::{sync::RwLock, task::JoinSet};
use tracing::{Instrument, info};

pub struct VanillaManager {
    manifest: OnceLock<Manifest>,
    version: RwLock<HashMap<Version, McVersion>>,
}

impl VanillaManager {
    pub fn new() -> Self {
        Self {
            manifest: OnceLock::new(),
            version: RwLock::new(HashMap::new()),
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait VanillaManage {
    async fn vanilla_manifest(&self) -> anyhow::Result<&Manifest>;
    async fn vanilla_version(&self, version: Version) -> anyhow::Result<McVersion>;
}

impl<T> VanillaManage for T
where
    T: AsRef<VanillaManager> + HttpRequest,
{
    async fn vanilla_manifest(&self) -> anyhow::Result<&Manifest> {
        if let Some(manifest) = self.as_ref().manifest.get() {
            return Ok(manifest);
        }
        info!("synchronizing minecraft version manifest");
        let manifest = self.http_get(VERSION_MANIFEST_URL).await?.json().await?;
        Ok(self.as_ref().manifest.get_or_init(|| manifest))
    }

    async fn vanilla_version(&self, version: Version) -> anyhow::Result<McVersion> {
        if let Some(mc_version) = self.as_ref().version.read().await.get(&version) {
            return Ok(mc_version.clone());
        }
        info!("synchronizing minecraft {version} version metadata");
        let manifest = self.vanilla_manifest().await?;
        let url = manifest
            .get_version(&version.to_string())
            .ok_or(anyhow!("minecraft version {version} not found in manifest"))?
            .url
            .to_owned();
        let mc_version = self.http_get(url).await?.json::<McVersion>().await?;
        self.as_ref()
            .version
            .write()
            .await
            .insert(version, mc_version.clone());
        Ok(mc_version)
    }
}

impl Creeper {
    pub async fn vanilla_lib(&self, version: Version) -> anyhow::Result<FileMap> {
        let version = self.vanilla_version(version).await?;

        let arts = version
            // libraries
            .libraries
            .into_iter()
            // apply the rules
            .filter(|x| {
                x.rules.as_ref().is_none_or(|x| {
                    x.iter().all(|x| {
                        if !x.features.is_empty() {
                            todo!("does not support rules with features")
                        }
                        let os = x.os.as_ref().is_none_or(check_os);
                        match x.action {
                            mc_launchermeta::version::rule::RuleAction::Allow => os,
                            mc_launchermeta::version::rule::RuleAction::Disallow => !os,
                        }
                    })
                })
            })
            // entries with artifacts to download
            .filter_map(|x| x.downloads)
            // flatten list of artifacts
            .flat_map(|x| {
                x.classifiers
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|(class, art)| if check_class(&class) { Some(art) } else { None })
                    .chain(x.artifact.into_iter())
            })
            // remove duplication
            .map(|x| {
                (
                    x.path,
                    Download {
                        sha1: x.sha1,
                        size: x.size,
                        url: x.url,
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        info!("downloading {} library artifacts", arts.len());

        let mut set = JoinSet::new();

        for (path, down) in arts {
            let creeper = self.clone();
            let fut = async move {
                creeper
                    .download(
                        path.clone(),
                        down.url,
                        Some(down.size),
                        Some(down.sha1).map(Checksum::sha1),
                    )
                    .await
                    .map(|x| (path, x))
            };
            set.spawn(fut.in_current_span());
        }

        let mut map = FileMap::new();

        while let Some(res) = set.join_next().await {
            let (path, art) = res??;
            map.insert(path.into(), art);
        }

        Ok(map)
    }
}
