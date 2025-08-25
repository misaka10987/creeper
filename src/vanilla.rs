use std::collections::HashMap;

use crate::{
    Checksum, Creeper,
    mc::{check_class, check_os},
};

use anyhow::anyhow;
use mc_launchermeta::{
    VERSION_MANIFEST_URL, version::Version as McVersion, version_manifest::Manifest,
};

use semver::Version;

use tokio::task::JoinSet;
use tracing::{Instrument, info};

impl Creeper {
    async fn fetch_manifest(&self) -> anyhow::Result<&Manifest> {
        info!("synchronizing minecraft version manifest");
        let manifest = self.http_get(VERSION_MANIFEST_URL).await?.json().await?;
        Ok(self.manifest.get_or_init(|| manifest))
    }

    pub async fn manifest(&self) -> anyhow::Result<&Manifest> {
        if let Some(manifest) = self.manifest.get() {
            return Ok(manifest);
        }
        self.fetch_manifest().await
    }

    async fn fetch_mc_version(&self, version: Version) -> anyhow::Result<McVersion> {
        info!("synchronizing minecraft {version} version metadata");
        let manifest = self.manifest().await?;
        let url = manifest
            .get_version(&version.to_string())
            .ok_or(anyhow!("minecraft version {version} not found in manifest"))?
            .url
            .to_owned();
        let mc_version = self.http_get(url).await?.json::<McVersion>().await?;
        self.mc_version
            .write()
            .await
            .insert(version, mc_version.clone());
        Ok(mc_version)
    }

    pub async fn mc_version(&self, version: Version) -> anyhow::Result<McVersion> {
        if let Some(mc_version) = self.mc_version.read().await.get(&version) {
            return Ok(mc_version.clone());
        }
        self.fetch_mc_version(version).await
    }

    pub async fn download_mc_lib(&self, version: Version) -> anyhow::Result<()> {
        let mc_version = self.mc_version(version).await?;

        let arts = mc_version
            // libraries
            .libraries
            .into_iter()
            // apply the rules
            .filter(|x| {
                x.rules.as_ref().is_none_or(|rules| {
                    rules.iter().all(|rule| {
                        if !rule.features.is_empty() {
                            todo!("does not support rules with features")
                        }
                        let os = rule.os.as_ref().is_none_or(check_os);
                        match rule.action {
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
            .map(|x| (x.sha1.clone(), x))
            .collect::<HashMap<_, _>>();

        info!("downloading {} library artifacts", arts.len());

        let mut set = JoinSet::new();

        for art in arts.into_values() {
            let creeper = self.clone();
            let fut = async move {
                creeper
                    .storage
                    .download(
                        art.path,
                        art.url,
                        Some(art.size),
                        Some(art.sha1).map(Checksum::sha1),
                    )
                    .await
            };
            set.spawn(fut.in_current_span());
        }

        while let Some(res) = set.join_next().await {
            res??;
        }

        Ok(())
    }
}
