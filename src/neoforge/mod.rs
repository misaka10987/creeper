mod client;
mod container;
mod fmt;
mod prelude;
mod server;
mod version;

use std::time::Duration;

use reqwest::Client;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::{
    Artifact, Checksum, Creeper,
    builtin::{
        SyncBuiltinIndex, fabric_id, neoforge_client_id, neoforge_id, neoforge_server_id,
        vanilla_id,
    },
    index::{Index, VersionRev},
    pack::PackNode,
};

pub use prelude::*;

pub struct NeoforgeManager {
    http: Client,
}

impl NeoforgeManager {
    pub fn new(http: Client) -> Self {
        Self { http }
    }
}

impl SyncBuiltinIndex for NeoforgeManager {
    fn package(&self) -> crate::prelude::Id {
        neoforge_id()
    }

    async fn sync_index(&self) -> anyhow::Result<Index> {
        let versions = query_neoforge_versions(&self.http).await?;

        let count = versions.len();

        let index = versions
            .into_iter()
            .filter_map(|s| s.parse::<NfVersion>().ok())
            .filter_map(|v| v.encode().ok())
            .map(|v| {
                let grp = [
                    (neoforge_client_id(), format!("={v}").parse().unwrap()),
                    (neoforge_server_id(), format!("={v}").parse().unwrap()),
                ];

                let conflict = [(fabric_id(), VersionReq::STAR)].into();

                let node = PackNode {
                    either_dep: vec![grp.into()],
                    conflict,
                    ..Default::default()
                };

                (VersionRev::new(v), node)
            })
            .collect::<Index>();

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

impl Creeper {
    async fn neoforge_installer_jar(&self, version: Version) -> anyhow::Result<Artifact> {
        let nf_version = NfVersion::decode(version);

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

        let sha1 = self
            .http
            .req()
            .await
            .get(sha1_url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?
            .trim()
            .into();

        let name = format!("neoforge-{nf_version}-installer.jar");
        let installer = self
            .download(name, url, None, [Checksum::sha1(sha1)])
            .await?;

        Ok(installer)
    }
}

pub fn nf_mc_req(version: &Version) -> Version {
    if version.major >= 26 {
        let high = version.patch >> 32;
        Version::new(version.major, version.minor, high)
    } else {
        Version::new(1, version.major, version.minor)
    }
}

pub fn mc_nf_req(version: &Version) -> VersionReq {
    if version.major < 26 {
        return format!("{}.{}.*", version.minor, version.patch)
            .parse()
            .unwrap();
    }

    // The higher half 32-bits of corresponding NeoForge version patch number shall match the Minecraft version patch number.
    let lower = version.patch << 32;
    let upper = (version.patch + 1) << 32;

    format!(
        ">={0}.{1}.{2}, <{0}.{1}.{3}",
        version.major, version.minor, lower, upper
    )
    .parse()
    .unwrap()
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
            let req = nf_mc_req(&version);
            let req = format!("={}", req).parse().unwrap();

            let dep = Some((vanilla_id(), req)).into_iter().collect();
            let node = PackNode {
                dep,
                ..Default::default()
            };
            (VersionRev::new(version), node)
        })
        .collect()
}
