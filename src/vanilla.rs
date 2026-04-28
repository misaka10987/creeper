use std::{collections::HashMap, path::PathBuf, sync::OnceLock};

use crate::{
    Artifact, Checksum, Creeper, Id, Install,
    index::{Index, IndexLine, VersionRev},
    mc::{check_class, check_os},
    pack::PackNode,
    path::creeper_cache_dir,
};

use anyhow::anyhow;
use mc_launchermeta::{
    VERSION_MANIFEST_URL,
    version::{
        Version as McVersion,
        library::{Artifact as McArtifact, Library},
    },
    version_manifest::Manifest,
};

use reqwest::Client;
use semver::Version;

use tokio::{sync::RwLock, task::JoinSet};
use tracing::{Instrument, debug, info, trace};

fn vanilla_index(versions: impl IntoIterator<Item = Version>) -> Index {
    versions
        .into_iter()
        .map(|version| {
            (
                VersionRev(version, 0),
                PackNode {
                    dep: HashMap::new(),
                },
            )
        })
        .collect()
}

pub struct VanillaManager {
    http: Client,
    manifest: OnceLock<Manifest>,
    version: RwLock<HashMap<Version, McVersion>>,
    index: OnceLock<Index>,
}

impl VanillaManager {
    pub fn new(http: Client) -> Self {
        Self {
            http,
            manifest: OnceLock::new(),
            version: RwLock::new(HashMap::new()),
            index: OnceLock::new(),
        }
    }

    fn index_cache_path() -> anyhow::Result<PathBuf> {
        let path = creeper_cache_dir()?
            .join("builtin")
            .join("index")
            .join(Id::vanilla().indexed_path())
            .with_added_extension("jsonl");
        Ok(path)
    }

    pub async fn update(&self) -> anyhow::Result<()> {
        info!("updating vanilla metadata");

        let req = self.http.get(VERSION_MANIFEST_URL).build()?;
        let res = self.http.execute(req).await?;

        let manifest = res.json::<Manifest>().await?;

        let mut versions = vec![];

        let count = manifest.versions.len();

        for version in manifest.versions {
            if let Some(version) = version.id.parse().ok() {
                versions.push(version);
            } else {
                trace!("ignoring invalid vanilla version {}", version.id);
            }
        }

        debug!(
            "retrieved {count} vanilla versions, of which {} valid",
            versions.len()
        );

        let index = vanilla_index(versions);

        let cache = Self::index_cache_path()?;
        IndexLine::write(cache, &Id::minecraft(), index).await?;

        Ok(())
    }

    pub fn blocking_get_index(&self) -> anyhow::Result<&Index> {
        if let Some(index) = self.index.get() {
            return Ok(index);
        }

        let cache = Self::index_cache_path()?;
        let index = IndexLine::blocking_read(cache)?;

        Ok(self.index.get_or_init(|| index))
    }
}

impl Creeper {
    pub async fn update_vanilla(&self) -> anyhow::Result<()> {
        self.vanilla.update().await
    }

    async fn vanilla_lib(&self, lib: Vec<Library>) -> anyhow::Result<Vec<Artifact>> {
        let arts = filter_lib(lib);

        info!("downloading {} library artifacts", arts.len());

        let mut set = JoinSet::new();

        for art in arts {
            let creeper = self.clone();
            let fut = async move {
                creeper
                    .download(
                        art.path,
                        art.url,
                        Some(art.size),
                        Some(Checksum::sha1(art.sha1)),
                    )
                    .await
            };
            set.spawn(fut.in_current_span());
        }

        let mut lib = vec![];

        while let Some(res) = set.join_next().await {
            lib.push(res??);
        }

        Ok(lib)
    }

    pub async fn list_vanilla_version(&self) -> anyhow::Result<Vec<Version>> {
        let manifest = self.vanilla_manifest().await?;
        let mut list = vec![];
        for v in &manifest.versions {
            list.push(v.id.parse()?);
        }
        Ok(list)
    }

    pub async fn vanilla_manifest(&self) -> anyhow::Result<&Manifest> {
        if let Some(manifest) = self.vanilla.manifest.get() {
            return Ok(manifest);
        }
        info!("synchronizing minecraft version manifest");

        let req = self.http.get(VERSION_MANIFEST_URL).build()?;
        let res = self.http.execute(req).await?;

        let manifest = res.json().await?;

        Ok(self.vanilla.manifest.get_or_init(|| manifest))
    }

    pub async fn vanilla_version(&self, version: Version) -> anyhow::Result<McVersion> {
        if let Some(mc_version) = self.vanilla.version.read().await.get(&version) {
            return Ok(mc_version.clone());
        }
        info!("synchronizing minecraft {version} version metadata");
        let manifest = self.vanilla_manifest().await?;
        let url = manifest
            .get_version(&version.to_string())
            .ok_or(anyhow!("minecraft version {version} not found in manifest"))?
            .url
            .to_owned();

        let req = self.http.get(url).build()?;
        let res = self.http.execute(req).await?;
        let mc_version = res.json::<McVersion>().await?;

        self.vanilla
            .version
            .write()
            .await
            .insert(version, mc_version.clone());
        Ok(mc_version)
    }

    pub async fn vanilla_install(&self, version: Version) -> anyhow::Result<Install> {
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

fn filter_lib(lib: Vec<Library>) -> Vec<McArtifact> {
    lib.into_iter()
        // apply the rules
        .filter(|x| {
            x.rules.iter().flatten().all(|x| {
                if !x.features.is_empty() {
                    todo!("does not support rules with features")
                }
                let apply = x.os.as_ref().is_none_or(check_os);
                match x.action {
                    mc_launchermeta::version::rule::RuleAction::Allow => apply,
                    mc_launchermeta::version::rule::RuleAction::Disallow => !apply,
                }
            })
        })
        // entries with artifacts to download
        .filter_map(|x| x.downloads)
        // flatten list of artifacts
        .flat_map(|x| {
            x.classifiers
                .into_iter()
                .flatten()
                .filter_map(|(class, art)| check_class(&class).then_some(art))
                .chain(x.artifact)
        })
        // deduplication
        .map(|x| (x.sha1.clone(), x))
        .collect::<HashMap<_, _>>()
        .into_iter()
        .map(|(_k, v)| v)
        .collect()
}
