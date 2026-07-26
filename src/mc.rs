use std::{
    collections::HashMap,
    ops::Deref,
    sync::{Arc, OnceLock},
    time::Duration,
};

use anyhow::anyhow;
use mc_launchermeta::{
    VERSION_MANIFEST_URL, version::Version as McVersion, version_manifest::Manifest,
};
use reqwest::Client;
use semver::Version;
use tokio::sync::RwLock;
use tracing::{info, trace};

use crate::{
    Id, VersionRev, builtin::SyncBuiltinIndex, index::Index, neoforge::mc_nf_req, pack::PackNode,
};

pub struct ManifestClientInner {
    http: Client,

    manifest: OnceLock<Manifest>,

    version_list: OnceLock<Vec<Version>>,

    version: RwLock<HashMap<Version, McVersion>>,
}

#[derive(Clone)]
pub struct ManifestClient(Arc<ManifestClientInner>);

impl Deref for ManifestClient {
    type Target = ManifestClientInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ManifestClient {
    pub fn new(http: Client) -> Self {
        let inner = ManifestClientInner {
            http,
            manifest: OnceLock::new(),
            version_list: OnceLock::new(),
            version: RwLock::new(HashMap::new()),
        };

        Self(Arc::new(inner))
    }

    pub async fn get_manifest(&self) -> anyhow::Result<&Manifest> {
        if let Some(manifest) = self.manifest.get() {
            trace!("using cached minecraft manifest");
            return Ok(manifest);
        }

        let manifest = self
            .http
            .get(VERSION_MANIFEST_URL)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(self.manifest.get_or_init(|| manifest))
    }

    pub async fn get_version_list(&self) -> anyhow::Result<&Vec<Version>> {
        if let Some(list) = self.version_list.get() {
            return Ok(list);
        }

        let manifest = self.get_manifest().await?;

        let list = manifest
            .versions
            .iter()
            .filter_map(|v| v.id.parse().ok())
            .collect::<Vec<_>>();

        info!(
            "retrieved {} valid minecraft versions out of {}",
            list.len(),
            manifest.versions.len()
        );

        Ok(self.version_list.get_or_init(|| list))
    }

    pub async fn get_version(&self, version: &Version) -> anyhow::Result<McVersion> {
        if let Some(mc_version) = self.version.read().await.get(version) {
            trace!("using cached minecraft `version.json` for {version}");
            return Ok(mc_version.clone());
        }

        info!("synchronizing minecraft `version.json` for {version}");

        let manifest = self.get_manifest().await?;

        let url = manifest
            .get_version(&version.to_string())
            .ok_or(anyhow!("minecraft version {version} not found in manifest"))?
            .url
            .to_owned();

        let mc_version = self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json::<McVersion>()
            .await?;

        self.version
            .write()
            .await
            .insert(version.clone(), mc_version.clone());

        Ok(mc_version)
    }
}

pub struct ServerManager {
    manifest: ManifestClient,
}

impl ServerManager {
    pub fn new(manifest: ManifestClient) -> Self {
        Self { manifest }
    }
}

impl SyncBuiltinIndex for ServerManager {
    fn package(&self) -> Id {
        Id::server()
    }

    async fn sync_index(&self) -> anyhow::Result<Index> {
        let versions = self.manifest.get_version_list().await?;

        let index = versions
            .into_iter()
            .map(|v| {
                let grp = [
                    (Id::vanilla_server(), format!("={v}").parse().unwrap()),
                    (Id::neoforge_server(), mc_nf_req(v)),
                    (
                        "server-provider".parse().unwrap(),
                        format!("={v}").parse().unwrap(),
                    ),
                ];

                let either_dep = vec![grp.into()];

                let node = PackNode {
                    either_dep,
                    ..Default::default()
                };

                (VersionRev::new(v.clone()), node)
            })
            .collect();

        Ok(index)
    }

    fn cache_expiry(&self) -> std::time::Duration {
        Duration::from_hours(72)
    }
}
