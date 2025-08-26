use std::{collections::HashMap, sync::OnceLock};

use crate::{
    Checksum, Install,
    http::HttpRequest,
    mc::{check_class, check_os},
    pack::FileMap,
    storage::StorageManage,
};

use anyhow::anyhow;
use mc_launchermeta::{
    VERSION_MANIFEST_URL,
    version::{Download, Version as McVersion, library::Library},
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

pub trait VanillaManage {
    fn vanilla_manifest(
        &self,
    ) -> impl std::future::Future<Output = anyhow::Result<&Manifest>> + Send;
    fn vanilla_version(
        &self,
        version: Version,
    ) -> impl std::future::Future<Output = anyhow::Result<McVersion>> + Send;
    fn vanilla_install(
        &self,
        version: Version,
    ) -> impl std::future::Future<Output = anyhow::Result<Install>> + Send;
}

trait VanillaManageImpl {
    fn vanilla_lib(
        &self,
        lib: Vec<Library>,
    ) -> impl std::future::Future<Output = anyhow::Result<FileMap>> + Send;
}

impl<T> VanillaManageImpl for T
where
    T: StorageManage + Clone + Send + Sync + 'static,
{
    async fn vanilla_lib(&self, lib: Vec<Library>) -> anyhow::Result<FileMap> {
        let arts = filter_lib(lib);

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

impl<T> VanillaManage for T
where
    T: AsRef<VanillaManager> + HttpRequest + StorageManage + Clone + Send + Sync + 'static,
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

    async fn vanilla_install(&self, version: Version) -> anyhow::Result<Install> {
        let version = self.vanilla_version(version).await?;
        let client = version.downloads.client;
        let client = self
            .download(
                "minecraft.jar".into(),
                client.url,
                Some(client.size),
                Some(Checksum::sha1(client.sha1)),
            )
            .await?;
        let lib = self.vanilla_lib(version.libraries).await?;
        let asset_index = version.asset_index;
        let asset_index = self
            .download(
                asset_index.id,
                asset_index.url,
                Some(asset_index.size),
                Some(Checksum::sha1(asset_index.sha1)),
            )
            .await?;
        let install = Install {
            java_lib: lib,
            java_main_class: Some(version.main_class),
            mc_jar: Some(client),
            mc_asset_index: Some(asset_index),
            ..Default::default()
        };
        Ok(install)
    }
}

fn filter_lib(lib: Vec<Library>) -> HashMap<String, Download> {
    lib.into_iter()
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
        .collect()
}
