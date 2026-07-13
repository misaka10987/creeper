use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env::consts::{ARCH, OS},
    iter::once,
    path::{Path, PathBuf},
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

    fn vanilla_args_install(&self, args: &mc_version::Arguments, version_name: &str) -> Install {
        let rule = RuleChecker::default();

        let version_type = format!("creeper {VERSION}");

        let vars = [
            ("version_name", version_name),
            ("game_directory", "."),
            ("version_type", &version_type),
            ("natives_directory", "./.creeper/native"),
            ("launcher_name", "creeper"),
            ("launcher_version", VERSION),
            ("library_directory", "./.creeper/lib"),
        ]
        .into_iter()
        .collect::<HashMap<_, _>>();

        let java_flag = args
            .jvm
            .iter()
            .filter_map(|a| a.rules.iter().all(rule.checker()).then_some(&a.values))
            .flatten();

        let java_flag = skip_two(
            |a| ["--class-path", "-cp", "--module-path", "-p"].contains(&a.as_str()),
            java_flag,
        );

        let java_flag = java_flag
            .iter()
            .map(|x| shellexpand::env_with_context_no_errors(x, |k| vars.get(k)).to_string())
            .collect();

        let mc_flag = args
            .game
            .iter()
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

        let mc_flag = mc_flag
            .iter()
            .map(|x| shellexpand::env_with_context_no_errors(x, |k| vars.get(k)).to_string())
            .collect();

        Install {
            java_flag,
            mc_flag,
            ..Default::default()
        }
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

        let install = self.vanilla_version_install(mc_version.into()).await?;

        Ok(install)
    }

    pub async fn vanilla_version_install(&self, version: McVersionExt) -> anyhow::Result<Install> {
        let mut install = Install::default();

        let rule = RuleChecker::default();

        if let Some(downloads) = version.downloads {
            let client = self
                .download(
                    format!("{}.jar", version.id),
                    downloads.client.url,
                    Some(downloads.client.size),
                    once(Checksum::sha1(downloads.client.sha1)),
                )
                .await?;

            install.extend(once(Install {
                mc_jar: Some(client),
                ..Default::default()
            }));
        }

        let lib = self.vanilla_lib(version.libraries).await?;

        let java_args = version
            .arguments
            .iter()
            .flat_map(|x| &x.jvm)
            .filter_map(|a| a.rules.iter().all(rule.checker()).then_some(&a.values))
            .flatten();

        let mut java_lib_mod = HashMap::new();

        for p in java_module_path(java_args.map(|a| a.as_str()))? {
            let path = Path::new(p);

            if let Some(art) = lib.get(path) {
                java_lib_mod.insert(path.into(), art.clone());
            }
        }

        if let Some(asset_index) = version.asset_index {
            let asset_index = self.download_asset_index(asset_index).await?;

            let asset = self.vanilla_asset_install(asset_index).await?;

            install.extend(once(asset));
        }

        if let Some(args) = version.arguments {
            let arg = self.vanilla_args_install(&args, &version.id);

            install.extend(once(arg));
        }

        install.extend(once(Install {
            java_lib_class: lib,
            java_lib_mod,
            java_main_class: Some(version.main_class),
            ..Default::default()
        }));

        Ok(install)
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

fn java_module_path<'a>(
    args: impl IntoIterator<Item = &'a str>,
) -> anyhow::Result<HashSet<&'a str>> {
    let mut it = args.into_iter().peekable();

    let mut p = HashSet::new();

    while let Some(arg) = it.next() {
        if !(arg == "--module-path" || arg == "-p") {
            continue;
        }

        let value = it
            .peek()
            .ok_or(anyhow!("missing value for java module path"))?;

        let paths = value
            .split("${classpath_separator}")
            .map(|x| x.strip_prefix("${library_directory}/").unwrap_or(x));

        p.extend(paths);
    }

    Ok(p)
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

    #[serde(rename = "type")]
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
