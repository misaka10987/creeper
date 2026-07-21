mod container;
mod fmt;
mod prelude;
mod server;

use std::{collections::HashMap, iter::once, path::PathBuf, str::FromStr, time::Duration};

use anyhow::anyhow;
use neoforge::NfInstallProfile;
use reqwest::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::{
    Artifact, Checksum, Creeper, Id, Install,
    builtin::SyncBuiltinIndex,
    index::{Index, VersionRev},
    pack::PackNode,
    path::creeper_cache_dir,
    zip::{extract_zip, extract_zip_to},
};

pub use prelude::*;

fn cache_path() -> anyhow::Result<PathBuf> {
    let path = creeper_cache_dir()?.join("builtin").join("neoforge");
    Ok(path)
}

pub struct NeoforgeManager {
    http: Client,
}

impl NeoforgeManager {
    pub fn new(http: Client) -> Self {
        Self { http }
    }
}

async fn query_neoforge_versions(http: &Client) -> anyhow::Result<Vec<String>> {
    const VERSIONS_URL: &str =
        "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    struct Versions {
        is_snapshot: bool,
        versions: Vec<String>,
    }

    let versions = http
        .get(VERSIONS_URL)
        .send()
        .await?
        .error_for_status()?
        .json::<Versions>()
        .await?;

    Ok(versions.versions)
}

impl SyncBuiltinIndex for NeoforgeManager {
    fn package(&self) -> Id {
        Id::neoforge()
    }

    async fn sync_index(&self) -> anyhow::Result<Index> {
        info!("updating NeoForge metadata");

        let versions = query_neoforge_versions(&self.http).await?;

        let count = versions.len();

        let versions = versions
            .into_iter()
            .filter_map(|s| parse_neoforge_version(&s));

        let index = neoforge_index(versions);

        debug!(
            "retrieved {count} NeoForge versions, of which {} valid",
            index.len()
        );

        Ok(index)
    }

    fn cache_expiry(&self) -> std::time::Duration {
        Duration::from_hours(72)
    }
}

impl Creeper {
    async fn neoforge_installer_jar(&self, version: &Version) -> anyhow::Result<Artifact> {
        let nf_version = decode_neoforge_version(version);

        let url = if self.config.use_bmclapi {
            format!(
                "https://bmclapi2.bangbang93.com/neoforge/version/{nf_version}/download/installer.jar"
            )
        } else {
            format!(
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/{nf_version}/neoforge-{nf_version}-installer.jar"
            )
        };

        let sha1_url = format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{nf_version}/neoforge-{nf_version}-installer.jar.sha1"
        );

        let req = self.http.get(sha1_url).build()?;
        let res = self.http.execute(req).await?;

        let sha1 = res.text().await?.trim().to_string();

        let name = format!("neoforge-{nf_version}-installer.jar");
        let installer = self
            .download(name, url, None, once(Checksum::sha1(sha1)))
            .await?;

        Ok(installer)
    }

    pub(crate) async fn neoforge_install(&self, version: &Version) -> anyhow::Result<Install> {
        let installer = self.neoforge_installer_jar(version).await?;

        let installer = self.retrieve_artifact(&installer).await?;

        // handle install as defined in `version.json`

        let mc_version = extract_zip(&installer, "version.json").await?;
        let mc_version = serde_json::from_str(&mc_version)?;

        let mut install = self.mc_version_install(mc_version).await?;

        // handle install as defined in `install_profile.json`

        let mut container =
            self.new_install_container(cache_path()?.join("tmp").join(version.to_string()));
        container.init().await?;

        let install_profile = extract_zip(&installer, "install_profile.json").await?;
        let install_profile = serde_json::from_str::<NfInstallProfile>(&install_profile)?;

        // libraries defined in `install_profile.json` does not require being prepended to `--module-path`
        // because they are loaded by neoforge's custom class loader
        let mut java_lib_file = self.vanilla_lib(install_profile.libraries).await?;

        container.add_lib_file(java_lib_file.clone());

        info!("preparing neoforge install environment");

        let vanilla_install = {
            // repeat code from [`Self::install`] to avoid async recursion
            let version = nf_required_mc_version(version);
            if let Some(install) = self
                .get_install_cache(&Id::vanilla(), &version.clone().into())
                .await?
            {
                install
            } else {
                let install = self.vanilla_install(&version).await?;
                self.set_install_cache(&Id::vanilla(), &version.into(), Some(&install))
                    .await?;
                install
            }
        };

        let mc_jar = vanilla_install
            .mc_jar
            .ok_or(anyhow!("missing minecraft jar in vanilla install"))?;
        let mc_jar = self.retrieve_artifact(&mc_jar).await?;

        // prepare variables
        let mut vars = install_profile
            .data
            .into_iter()
            .map(|(k, v)| (k, v.client))
            .chain(once(("SIDE".into(), "client".into())))
            .chain(once(("MINECRAFT_JAR".into(), mc_jar.display().to_string())))
            .collect::<HashMap<_, _>>();

        // special case: BINPATCH /data/client.lzma is packaged in the installer jar
        // extract it first
        let binpatch = container
            .path()
            .join(".installer")
            .join("data")
            .join("client.lzma");
        extract_zip_to(&installer, "data/client.lzma", &binpatch).await?;
        vars.insert("BINPATCH".into(), binpatch.display().to_string());

        container.add_var(vars);
        container.deploy_lib().await?;

        info!("running neoforge install processors");

        for proc in install_profile.processors {
            if !proc
                .sides
                .as_ref()
                .is_none_or(|x| x.contains(&"client".into()))
            {
                debug!("skipping a processor because side mismatch: {proc}");
                continue;
            }

            container.run(&proc).await?;
        }

        info!("collecting neoforge install result");

        let collect = container
            .collect_lib_file(
                java_lib_file
                    .keys()
                    .chain(install.java_lib_class.keys())
                    .chain(install.java_lib_mod.keys())
                    .chain(install.java_lib_file.keys())
                    .chain(vanilla_install.java_lib_class.keys())
                    .chain(vanilla_install.java_lib_mod.keys())
                    .chain(vanilla_install.java_lib_file.keys())
                    .map(|k| k.as_path()),
            )
            .await?;

        container.deinit().await?;

        java_lib_file.extend(collect);

        install.extend(once(Install {
            java_lib_file,
            ..Default::default()
        }));

        install.simplify();

        install.disable_mc_jar = true;

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

fn nf_required_mc_version(version: &Version) -> Version {
    if version.major >= 26 {
        let high = version.patch >> 32;
        Version::new(version.major, version.minor, high)
    } else {
        Version::new(1, version.major, version.minor)
    }
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
            let req = nf_required_mc_version(&version);
            let req = format!("={}", req).parse().unwrap();

            let dep = Some((Id::vanilla(), req)).into_iter().collect();
            let node = PackNode {
                dep,
                ..Default::default()
            };
            (VersionRev::new(version), node)
        })
        .collect()
}
