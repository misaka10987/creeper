use std::{collections::BTreeSet, path::PathBuf, str::FromStr, sync::OnceLock};

use reqwest::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::{
    Creeper, Id,
    index::{Index, IndexLine, VersionRev},
    pack::PackNode,
    path::creeper_cache_dir,
};

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
            let dep = Some((Id::minecraft(), req.parse().unwrap()))
                .into_iter()
                .collect();
            let node = PackNode { dep };
            (VersionRev(version, 0), node)
        })
        .collect()
}
