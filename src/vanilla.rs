use std::{
    collections::{BTreeMap, HashMap},
    env::consts::{ARCH, OS},
    iter::once,
    path::PathBuf,
    sync::OnceLock,
    time::Duration,
};

use crate::{
    Artifact, Checksum, Creeper, Id, Install, MavenCoord, VERSION,
    builtin::{SyncBuiltinIndex, UpdateIndex},
    index::{Index, VersionRev},
    pack::PackNode,
    util::skip_two,
};

use anyhow::anyhow;
use mc_launchermeta::{
    VERSION_MANIFEST_URL, version as mc_version,
    version::{
        Version as McVersion,
        library::{Artifact as McArtifact, Library},
        rule::{Os, Rule},
    },
    version_manifest::Manifest,
};

use reqwest::Client;
use semver::Version;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info, trace};

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

pub fn check_class(class: &str) -> bool {
    match class {
        "natives-linux" => OS == "linux",
        "natives-windows" => OS == "windows",
        "natives-macos" | "natives-osx" => OS == "macos",
        c => todo!("unknown classifier {c}"),
    }
}

#[derive(Default)]
pub struct RuleChecker {
    feature: HashMap<String, bool>,
}

impl RuleChecker {
    pub fn checker(&self) -> impl Fn(&Rule) -> bool {
        move |rule| self.check(rule)
    }

    pub fn check(&self, rule: &Rule) -> bool {
        let os = rule.os.as_ref().is_none_or(Self::check_os);

        let feature = rule.features.iter().all(|(k, v)| {
            let enable = self.feature.get(k).unwrap_or(&false);

            enable == v
        });

        let apply = os && feature;

        match rule.action {
            mc_launchermeta::version::rule::RuleAction::Allow => apply,
            mc_launchermeta::version::rule::RuleAction::Disallow => !apply,
        }
    }

    pub fn check_os(os: &Os) -> bool {
        let name = os.name.as_ref().is_none_or(|x| match x {
            mc_launchermeta::version::rule::OsName::Windows => OS == "windows",
            mc_launchermeta::version::rule::OsName::Osx => OS == "macos",
            mc_launchermeta::version::rule::OsName::Linux => OS == "linux",
        });

        let arch = os.arch.as_ref().is_none_or(|x| match x {
            mc_launchermeta::version::rule::OsArch::X86 => ARCH == "x86" || ARCH == "x86_64",
        });

        let version = os
            .version
            .as_ref()
            .is_none_or(|_| todo!("does not support checking OS version"));

        name && arch && version
    }
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

    fn cache_expiry(&self) -> std::time::Duration {
        Duration::from_hours(72)
    }
}

impl Creeper {
    pub async fn update_vanilla(&self) -> anyhow::Result<()> {
        if self.args.offline {
            info!("skipping vanilla update because offline mode enabled");
            return Ok(());
        }

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
        let mut install = Install::default();

        let rule = RuleChecker::default();

        let mc_version = self.vanilla_version(version.clone()).await?;

        let client = mc_version.downloads.client;

        let url = if self.config.use_bmclapi {
            format!("https://bmclapi2.bangbang93.com/version/{version}/client")
        } else {
            client.url
        };

        let client = self
            .download(
                "minecraft.jar".into(),
                url,
                Some(client.size),
                Some(Checksum::sha1(client.sha1)),
            )
            .await?;
        let lib = self.vanilla_lib(mc_version.libraries).await?;

        let asset_index = self.download_asset_index(mc_version.asset_index).await?;

        let asset = self.vanilla_asset_install(asset_index).await?;

        install.extend(once(asset));

        let mc_flag = mc_version
            .arguments
            .iter()
            .map(|x| &x.game)
            .flatten()
            .filter_map(|a| a.rules.iter().all(rule.checker()).then_some(&a.values))
            .flatten();

        let mc_flag = skip_two(
            |a| {
                [
                    "--username",
                    "--assetsDir",
                    "--assetIndex",
                    "--uuid",
                    "--accessToken",
                    "--userType",
                ]
                .contains(&a.as_str())
            },
            mc_flag,
        );

        let vars = [
            ("version_name", version.to_string()),
            ("game_directory", ".".into()),
            ("version_type", format!("creeper {VERSION}")),
        ]
        .into_iter()
        .collect::<HashMap<_, _>>();

        let mc_flag = mc_flag
            .into_iter()
            .map(|x| shellexpand::env_with_context_no_errors(x, |k| vars.get(k)).to_string())
            .collect();

        let java_flag = mc_version
            .arguments
            .iter()
            .map(|x| &x.jvm)
            .flatten()
            .filter_map(|a| a.rules.iter().all(rule.checker()).then_some(&a.values))
            .flatten();

        let java_flag = skip_two(|x| *x == "-cp", java_flag);

        let vars = [
            ("natives_directory", "./.creeper/native"),
            ("launcher_name", "creeper"),
            ("launcher_version", VERSION),
        ]
        .into_iter()
        .collect::<HashMap<_, _>>();

        let java_flag = java_flag
            .into_iter()
            .map(|x| shellexpand::env_with_context_no_errors(x, |k| vars.get(k)).to_string())
            .collect();

        install.extend(once(Install {
            java_lib_class: lib,
            java_main_class: Some(mc_version.main_class),
            java_flag,
            mc_jar: Some(client),
            mc_flag,
            ..Default::default()
        }));

        Ok(install)
    }

    pub async fn vanilla_version_install(&self, version: &Version) -> anyhow::Result<Install> {
        todo!()
    }
}

fn filter_lib(lib: impl IntoIterator<Item = Library>) -> Vec<McArtifact> {
    let rule = RuleChecker::default();

    lib.into_iter()
        // apply the rules
        .filter(|x| x.rules.iter().flatten().all(rule.checker()))
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
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McVersionExt {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<mc_version::Arguments>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub minecraft_arguments: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_index: Option<mc_version::AssetIndex>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub assets: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub compliance_level: Option<u8>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloads: Option<mc_version::Downloads>,

    pub id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_version: Option<mc_version::JavaVersion>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<mc_version::library::Library>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<mc_version::logging::Logging>,

    pub main_class: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_launcher_version: Option<u8>,

    pub release_time: String,

    pub time: String,

    pub kind: mc_launchermeta::VersionKind,
}

impl From<mc_version::Version> for McVersionExt {
    fn from(value: mc_version::Version) -> Self {
        Self {
            inherits_from: None,
            arguments: value.arguments,
            minecraft_arguments: value.minecraft_arguments,
            asset_index: Some(value.asset_index),
            assets: Some(value.assets),
            compliance_level: value.compliance_level,
            downloads: Some(value.downloads),
            id: value.id,
            java_version: value.java_version,
            libraries: value.libraries,
            logging: value.logging,
            main_class: value.main_class,
            minimum_launcher_version: Some(value.minimum_launcher_version),
            release_time: value.release_time,
            time: value.time,
            kind: value.kind,
        }
    }
}
