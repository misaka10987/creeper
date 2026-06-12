use std::{
    collections::{BTreeSet, HashMap},
    iter::once,
    path::PathBuf,
    str::FromStr,
    sync::OnceLock,
};

use anyhow::anyhow;
use mc_launchermeta::{
    VersionKind,
    version::{Arguments, JavaVersion, library::Library, logging::Logging},
};
use reqwest::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::{debug, error, info, trace};
use url::Url;

use crate::{
    Artifact, Checksum, Creeper, Id, Install, MavenCoord,
    index::{Index, IndexLine, VersionRev},
    pack::PackNode,
    path::creeper_cache_dir,
    util::{JarManifest, extract_zip},
};

fn classpath_path(cp: &str) -> PathBuf {
    todo!()
}

const VERSIONS_URL: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

pub struct NeoforgeManager {
    http: Client,
    versions: OnceLock<BTreeSet<Version>>,
    index: OnceLock<Index>,
}

impl NeoforgeManager {
    pub fn new(http: Client) -> Self {
        Self {
            http,
            versions: OnceLock::new(),
            index: OnceLock::new(),
        }
    }

    pub fn index_cache_path() -> anyhow::Result<PathBuf> {
        let path = creeper_cache_dir()?
            .join("index")
            .join(Id::neoforge().indexed_path())
            .with_added_extension("jsonl");
        Ok(path)
    }

    pub async fn update(&self) -> anyhow::Result<()> {
        info!("updating NeoForge metadata");

        let req = self.http.get(VERSIONS_URL).build()?;
        let res = self.http.execute(req).await?;

        #[derive(Clone, Debug, Serialize, Deserialize)]
        struct Versions {
            #[serde(rename = "isSnapshot")]
            is_snapshot: bool,
            versions: Vec<String>,
        }

        let versions = res.json::<Versions>().await?;

        let count = versions.versions.len();

        let versions = versions
            .versions
            .into_iter()
            .filter_map(|s| parse_neoforge_version(&s));

        let index = neoforge_index(versions);

        debug!(
            "retrieved {count} NeoForge versions, of which {} valid",
            index.len()
        );

        let cache = Self::index_cache_path()?;

        IndexLine::write(&cache, &Id::neoforge(), index).await?;

        Ok(())
    }

    pub async fn list_version(&self) -> anyhow::Result<&BTreeSet<Version>> {
        if let Some(versions) = self.versions.get() {
            return Ok(versions);
        }

        let versions = self
            .get_index()
            .await?
            .keys()
            .map(|VersionRev(v, _)| v)
            .cloned()
            .collect();

        Ok(self.versions.get_or_init(|| versions))
    }

    pub async fn get_index(&self) -> anyhow::Result<&Index> {
        if let Some(index) = self.index.get() {
            return Ok(index);
        }

        let cache = Self::index_cache_path()?;

        let index = IndexLine::read(cache).await?;

        Ok(self.index.get_or_init(|| index))
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
    pub async fn list_neoforge_version(&self) -> anyhow::Result<&BTreeSet<Version>> {
        self.neoforge.list_version().await
    }

    pub async fn get_neoforge_index(&self) -> anyhow::Result<&Index> {
        self.neoforge.get_index().await
    }

    pub async fn update_neoforge(&self) -> anyhow::Result<()> {
        self.neoforge.update().await
    }

    async fn neoforge_installer_jar(&self, version: &Version) -> anyhow::Result<Artifact> {
        let nf_version = decode_neoforge_version(version);

        let url = format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{nf_version}/neoforge-{nf_version}-installer.jar"
        );

        let sha1_url = format!("{url}.sha1");

        let req = self.http.get(sha1_url).build()?;
        let res = self.http.execute(req).await?;

        let sha1 = res.text().await?.trim().to_string();

        let name = format!("neoforge-{nf_version}-installer.jar");
        let installer = self
            .download(name, url, None, once(Checksum::sha1(sha1)))
            .await?;

        Ok(installer)
    }

    pub async fn neoforge_install(&self, version: &Version) -> anyhow::Result<Install> {
        let installer = self.neoforge_installer_jar(version).await?;

        let installer = self.retrieve_artifact(&installer).await?;

        // handle install as defined in `version.json`

        let version = extract_zip(&installer, "version.json").await?;
        let version = serde_json::from_str::<NfVersion>(&version)?;

        let mut install = self.neoforge_version_install(version).await?;

        // handle install as defined in `install_profile.json`

        let install_profile = extract_zip(&installer, "install_profile.json").await?;
        let install_profile = serde_json::from_str::<NfInstallProfile>(&install_profile)?;

        let mut java_lib_file = HashMap::new();

        // libraries defined in `install_profile.json` does not require being prepended to `--module-path`
        // because they are loaded by neoforge's custom class loader
        java_lib_file.extend(self.vanilla_lib(install_profile.libraries).await?);

        install.extend(once(Install {
            java_lib_file,
            ..Default::default()
        }));

        // TODO: run processors

        for proc in install_profile.processors {
            if proc
                .sides
                .is_some_and(|s| s.iter().find(|s| *s == "client").is_none())
            {
                trace!("skipping a processor because of side mismatch");
                continue;
            }

            let jar = install
                .java_lib_file
                .get(&proc.jar.parse::<MavenCoord>()?.path())
                .ok_or(anyhow!(
                    "processor runs {} but the jar file not found",
                    proc.jar
                ))?;

            let jar = self.retrieve_artifact(jar).await?;

            let manifest = extract_zip(&jar, "META-INF/MANIFEST.MF")
                .await?
                .parse::<JarManifest>()?;

            let main_class = manifest
                .main_class
                .ok_or(anyhow!("processor missing java main class"))?;

            let mut cp = vec![jar.display().to_string()];

            for c in proc.classpath {
                let coord = c.parse::<MavenCoord>()?;

                let jar = install.java_lib_file.get(&coord.path()).ok_or(anyhow!(
                    "processor classpath {} not found in java libraries",
                    c
                ))?;

                let jar = self.retrieve_artifact(jar).await?;

                cp.push(jar.display().to_string());
            }

            let mut cmd = Command::new("java");

            cmd.arg("--class-path").arg(cp.join(":"));

            cmd.arg(main_class);

            todo!("run {cmd:?}");
        }

        Ok(install)
    }

    async fn neoforge_version_install(&self, version: NfVersion) -> anyhow::Result<Install> {
        let java_lib_class = self.vanilla_lib(version.libraries).await?;
        let mut java_lib_mod = HashMap::new();

        let (java_flag, mc_flag) = if let Some(args) = version.arguments {
            let java_flag = args.jvm.into_iter().flat_map(|arg| arg.values);

            let mut it = java_flag.peekable();

            let mut java_flag = vec![];

            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "-p" => {
                        let value = it
                            .peek()
                            .ok_or(anyhow!("missing value for java argument -p"))?;

                        let value = value.replace("${library_directory}/", "");

                        let names = value.split("${classpath_separator}");

                        for name in names {
                            let path = PathBuf::from(name);
                            if let Some(art) = java_lib_class.get(&path) {
                                java_lib_mod.insert(path, art.clone());
                            } else {
                                error!("library {name} not found during neoforge install");
                            }
                        }

                        it.next();
                    }
                    s if !s.contains("$") => java_flag.push(s.into()),
                    s => error!("ignoring unsupported neoforge java argument {s}"),
                }
            }

            let mc_flag = args.game.into_iter().flat_map(|arg| arg.values).collect();

            (java_flag, mc_flag)
        } else {
            (vec![], vec![])
        };

        let install = Install {
            java_lib_class,
            java_lib_mod,
            java_main_class: Some(version.main_class),
            java_flag,
            mc_flag,
            ..Default::default()
        };

        Ok(install)
    }
}

/// NeoForge's versioning scheme does not always follow the semver standard:
///
/// - snapshots like `0.25w14craftmine.3-beta`;
///
/// - since minecraft 26, neoforge uses four components in its version number, like `26.1.0.0`.
///
/// This function attempts to parse a neoforge version following the semver standard.
/// If this fails, we will assume the version has four components,
/// and map the third and fourth component to the high and low 32-bits of patch number,
/// then parse the version again under the semver standard.
/// If all parsing attempts fail, will return `None`.
pub fn parse_neoforge_version(version: &str) -> Option<Version> {
    if let Ok(version) = version.parse() {
        return Some(version);
    }
    let (major, rest) = version.split_once('.')?;
    let rest = Version::from_str(rest).ok()?;
    let minor = rest.major;
    // since minecraft 26.*, neoforge has four version components, but semver only has three
    // we map the thrid component to the high 32-bits of the patch version, and the fourth component to the low 32-bits
    let (high, low) = (rest.minor, rest.patch);
    if high > u32::MAX as u64 || low > u32::MAX as u64 {
        return None;
    }
    let patch = (high << 32) | low;
    let mut version = rest.clone();
    version.major = major.parse().ok()?;
    version.minor = minor;
    version.patch = patch;
    Some(version)
}

pub fn decode_neoforge_version(version: &Version) -> String {
    if version.major < 26 {
        return version.to_string();
    }
    let high = version.patch >> 32;
    let low = version.patch & 0xFFFFFFFF;
    let pre = if version.pre.is_empty() {
        "".to_string()
    } else {
        format!("-{}", version.pre)
    };
    let build = if version.build.is_empty() {
        "".to_string()
    } else {
        format!("+{}", version.build)
    };

    let version = format!("{}.{}.{}.{}", version.major, version.minor, high, low);
    let version = format!("{}{}{}", version, pre, build);
    version
}

/// Generate NeoForge package index from list of versions, applying the following rules to each version:
///
/// - Package ID be `neoforge`;
///
/// - Version be the given version;
///
/// - Revision be `0`;
///
/// - For neoforge `x.y.z.w` where `x` >= 26, depend on `minecraft = ^x.y`; and
///
/// - For neoforge `x.y.z` where `x` < 26, depend on `minecraft = ^1.x.y`.
///
/// # Note
///
/// The behavior is undefined unless there is no duplicate version in the input.
fn neoforge_index(versions: impl IntoIterator<Item = Version>) -> Index {
    versions
        .into_iter()
        .map(|version| {
            let req = if version.major >= 26 {
                format!("{}.{}", version.major, version.minor)
            } else {
                format!("1.{}.{}", version.major, version.minor)
            };
            let dep = Some((Id::vanilla(), req.parse().unwrap()))
                .into_iter()
                .collect();
            let node = PackNode { dep };
            (VersionRev(version, 0), node)
        })
        .collect()
}

// mc_launchermeta::version::Version
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct NfVersion {
    //
    pub inherits_from: String,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
    // pub asset_index: AssetIndex,
    // pub assets: String,
    #[serde(default)]
    pub compliance_level: Option<u8>,
    // pub downloads: Downloads,
    pub id: String,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub logging: Option<Logging>,
    pub main_class: String,
    // pub minimum_launcher_version: u8,
    pub release_time: String,
    pub time: String,
    #[serde(rename = "type")]
    pub kind: VersionKind,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct NfInstallProfile {
    pub spec: u64,
    pub profile: String,
    pub version: String,
    pub icon: String,
    pub minecraft: Version,
    pub json: PathBuf,
    pub logo: PathBuf,
    pub welcome: String,
    pub mirror_list: Url,
    pub hide_extract: bool,
    pub data: HashMap<String, install_profile::DataValue>,
    pub processors: Vec<install_profile::Processor>,
    pub libraries: Vec<Library>,
    pub server_jar_path: String,
}

pub mod install_profile {
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    #[serde(deny_unknown_fields)]
    pub struct DataValue {
        pub client: String,
        pub server: String,
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    #[serde(deny_unknown_fields)]
    pub struct Processor {
        pub sides: Option<Vec<String>>,
        pub jar: String,
        pub classpath: Vec<String>,
        pub args: Vec<String>,
    }
}
