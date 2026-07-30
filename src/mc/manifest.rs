use std::{
    collections::HashMap,
    ops::Deref,
    sync::{Arc, OnceLock},
};

use anyhow::anyhow;
use mc_launchermeta::{
    VERSION_MANIFEST_URL, version::Version as McVersion, version_manifest::Manifest,
};
use semver::Version;
use tokio::sync::RwLock;
use tracing::{info, trace};

use crate::http::HttpThrottle;

pub struct ManifestClientInner {
    http: HttpThrottle,

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
    pub fn new(http: HttpThrottle) -> Self {
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
            .req()
            .await
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
            .req()
            .await
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
