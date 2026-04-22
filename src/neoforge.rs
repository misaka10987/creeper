use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
    sync::OnceLock,
};

use reqwest::Client;
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{Creeper, Id, pack::PackNode, registry::VersionRev};

const VERSIONS_URL: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

pub struct NeoforgeManager {
    http: Client,
    versions: OnceLock<BTreeSet<Version>>,
}

impl NeoforgeManager {
    pub fn new(http: Client) -> Self {
        Self {
            http,
            versions: OnceLock::new(),
        }
    }

    pub async fn list_version(&self) -> anyhow::Result<&BTreeSet<Version>> {
        if let Some(versions) = self.versions.get() {
            return Ok(versions);
        }

        let req = self.http.get(VERSIONS_URL).build()?;
        let res = self.http.execute(req).await?;

        #[derive(Clone, Debug, Serialize, Deserialize)]
        struct Versions {
            #[serde(rename = "isSnapshot")]
            is_snapshot: bool,
            versions: Vec<String>,
        }

        let versions = res.json::<Versions>().await?;

        let versions = versions
            .versions
            .into_iter()
            .filter_map(|s| parse_neoforge_version(&s));

        let versions = versions.into_iter().collect();

        Ok(self.versions.get_or_init(|| versions))
    }

    pub async fn get_index(&self) -> anyhow::Result<BTreeMap<VersionRev, PackNode>> {
        let list = self.list_version().await?;
        let index = list
            .iter()
            .map(|version| {
                let req = if version.major >= 26 {
                    format!("{}.{}", version.major, version.minor)
                } else {
                    format!("1.{}.{}", version.major, version.minor)
                };
                let dep = Some((Id::minecraft(), req.parse().unwrap()))
                    .into_iter()
                    .collect();
                let node = PackNode { dep };
                (VersionRev(version.clone(), 0), node)
            })
            .collect();
        Ok(index)
    }
}

impl Creeper {
    pub async fn list_neoforge_version(&self) -> anyhow::Result<&BTreeSet<Version>> {
        self.neoforge.list_version().await
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
