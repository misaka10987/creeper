use std::{
    collections::{BTreeMap, HashMap},
    iter::once,
    path::PathBuf,
    sync::OnceLock,
};

use crate::{
    Artifact, Checksum, Creeper, Id, Install, MavenCoord,
    builtin::{SyncBuiltinIndex, UpdateIndex},
    index::{Index, VersionRev},
    mc::{check_class, check_os},
    pack::PackNode,
};

use anyhow::{anyhow, ensure};
use mc_launchermeta::{
    VERSION_MANIFEST_URL,
    version::{
        Version as McVersion,
        library::{Artifact as McArtifact, Library},
        rule::Rule,
    },
    version_manifest::Manifest,
};

use reqwest::Client;
use semver::Version;

use serde::{Deserialize, Serialize};
use tokio::{fs::read_to_string, sync::RwLock};
use tracing::{debug, error, info, trace};

fn vanilla_index(versions: impl IntoIterator<Item = Version>) -> Index {
    versions
        .into_iter()
        .map(|version| {
            (
                VersionRev::new(version),
                PackNode {
                    dep: BTreeMap::new(),
                    ..Default::default()
                },
            )
        })
        .collect()
}

pub struct VanillaManager {
    http: Client,
    manifest: OnceLock<Manifest>,
    version: RwLock<HashMap<Version, McVersion>>,
}

impl VanillaManager {
    pub fn new(http: Client) -> Self {
        Self {
            http,
            manifest: OnceLock::new(),
            version: RwLock::new(HashMap::new()),
        }
    }
}

impl SyncBuiltinIndex for VanillaManager {
    fn package(&self) -> Id {
        Id::vanilla()
    }

    async fn sync_index(&self) -> anyhow::Result<Index> {
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

        Ok(index)
    }
}

impl Creeper {
    pub async fn update_vanilla(&self) -> anyhow::Result<()> {
        self.vanilla.update_index().await
    }

    pub(crate) async fn vanilla_lib(
        &self,
        lib: impl IntoIterator<Item = Library>,
    ) -> anyhow::Result<HashMap<PathBuf, Artifact>> {
        let arts = filter_lib(lib);

        info!("downloading {} library artifacts", arts.len());

        let lib = arts
            .into_iter()
            .map(|a| {
                let name = MavenCoord::from_path(&a.path)
                    .map(|c| c.to_string())
                    .unwrap_or(a.path.clone());

                (
                    a.path.into(),
                    (name, a.url, Some(a.size), once(Checksum::sha1(a.sha1))),
                )
            })
            .collect();

        let map = self.batch_download(lib).await?;

        Ok(map)
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

    pub(crate) async fn vanilla_install(&self, version: &Version) -> anyhow::Result<Install> {
        let mc_version = self.vanilla_version(version.clone()).await?;

        let client = mc_version.downloads.client;
        let client = self
            .download(
                "minecraft.jar".into(),
                client.url,
                Some(client.size),
                Some(Checksum::sha1(client.sha1)),
            )
            .await?;
        let lib = self.vanilla_lib(mc_version.libraries).await?;

        let asset_index = mc_version.asset_index;
        let asset_index = self
            .download(
                asset_index.id,
                asset_index.url,
                Some(asset_index.size),
                Some(Checksum::sha1(asset_index.sha1)),
            )
            .await?;

        let asset_index = self.retrieve_artifact(&asset_index).await?;
        let json = read_to_string(asset_index).await?;

        let asset_index = serde_json::from_str::<AssetIndex>(&json)?;
        let mc_asset = self.download_mc_asset(asset_index).await?;

        let install = Install {
            java_lib_class: lib,
            java_main_class: Some(mc_version.main_class),
            mc_jar: Some(client),
            mc_asset,
            mc_flag: vec!["--version".into(), version.to_string()],
            ..Default::default()
        };
        Ok(install)
    }

    async fn download_mc_asset(
        &self,
        index: AssetIndex,
    ) -> anyhow::Result<HashMap<PathBuf, Artifact>> {
        let mut map = HashMap::new();

        for (path, obj) in index.objects {
            let name = PathBuf::from(".minecraft")
                .join("assets")
                .join(&path)
                .display()
                .to_string();

            let src = asset_url(&obj.sha1)?;

            map.insert(
                path,
                (name, src, Some(obj.size), once(Checksum::sha1(obj.sha1))),
            );
        }

        let map = self.batch_download(map).await?;

        Ok(map)
    }
}

fn asset_url(sha1: &str) -> anyhow::Result<String> {
    ensure!(sha1.len() == 40, "invalid sha1 length");
    let first2 = &sha1[0..2];
    let url = format!("https://resources.download.minecraft.net/{first2}/{sha1}");
    Ok(url)
}

pub fn check_rule(rule: &Rule) -> bool {
    if !rule.features.is_empty() {
        error!("does not support checking rules with features")
    }

    let apply = rule.os.as_ref().is_none_or(check_os);

    match rule.action {
        mc_launchermeta::version::rule::RuleAction::Allow => apply,
        mc_launchermeta::version::rule::RuleAction::Disallow => !apply,
    }
}

fn filter_lib(lib: impl IntoIterator<Item = Library>) -> Vec<McArtifact> {
    lib.into_iter()
        // apply the rules
        .filter(|x| x.rules.iter().flatten().all(check_rule))
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

#[derive(Clone, Serialize, Deserialize)]
pub struct AssetIndex {
    pub objects: HashMap<PathBuf, Object>,
}

impl AssetIndex {
    pub fn from_map(map: HashMap<PathBuf, Artifact>) -> anyhow::Result<Self> {
        let mut objects = HashMap::new();

        for (path, art) in map {
            let sha1 = art
                .sha1
                .ok_or(anyhow!("missing SHA-1 checksum in asset index"))?;
            objects.insert(
                path,
                Object {
                    sha1,
                    size: art.len,
                },
            );
        }

        Ok(Self { objects })
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Object {
    #[serde(rename = "hash")]
    pub sha1: String,
    pub size: u64,
}
