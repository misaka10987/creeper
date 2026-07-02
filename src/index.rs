use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::BufRead,
    path::Path,
    str::FromStr,
    sync::RwLock,
};

use anyhow::{anyhow, bail, ensure};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use tokio::{
    fs::{File, create_dir_all, read_dir, read_to_string},
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter},
};
use tracing::debug;

use crate::{
    Creeper, Id, Package,
    builtin::{BlockingGetIndex, GetIndex},
    pack::PackNode,
};

pub type Index = BTreeMap<VersionRev, PackNode>;

pub async fn compile_index(src: impl AsRef<Path>) -> anyhow::Result<Index> {
    let src = src.as_ref();
    let id = src
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or(anyhow!(
            "invalid source {}, expected a directory named after package id",
            src.display()
        ))
        .and_then(Id::from_str)?;
    let mut index = Index::new();

    let mut read = read_dir(src).await?;
    while let Some(v) = read.next_entry().await? {
        let version = v
            .file_name()
            .to_str()
            .ok_or(anyhow!("invalid filename {}", v.path().display()))?
            .parse::<Version>()?;

        let mut read = read_dir(v.path()).await?;
        while let Some(r) = read.next_entry().await? {
            let path = r.path();
            if !r.file_type().await?.is_file() {
                bail!(
                    "invalid item {} in package registry, expected file",
                    path.display()
                );
            }
            if !path.extension().is_some_and(|s| s == "toml") {
                bail!(
                    "invalid item {} in package registry, expected TOML",
                    path.display()
                );
            }

            let rev = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or(anyhow!("failed to parse filename: {}", path.display()))?
                .parse::<u32>()?;

            let toml = read_to_string(&path).await?;
            let pack = toml::from_str::<Package>(&toml)?;

            ensure!(pack.id == id, "inconsistent id in {}", path.display());
            ensure!(
                pack.version == version,
                "inconsistent version in {}",
                path.display()
            );
            ensure!(
                pack.rev == rev,
                "inconsistent revision in {}",
                path.display()
            );

            index.insert(VersionRev::with_rev(version.clone(), rev), pack.node);
        }
    }

    Ok(index)
}

#[serde_inline_default]
#[derive(Clone, Serialize, Deserialize)]
pub struct IndexLine {
    pub id: Id,
    pub version: Version,
    #[serde_inline_default(0)]
    #[serde(skip_serializing_if = "is_zero")]
    pub rev: u32,
    #[serde(flatten)]
    pub node: PackNode,
}

#[allow(unused)] // used by `#[serde(skip_serializing_if = "is_zero")]`
fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl IndexLine {
    pub fn blocking_read(jsonl: impl AsRef<Path>) -> anyhow::Result<Index> {
        debug!("reading index from {}", jsonl.as_ref().display());

        let file = std::fs::File::options()
            .read(true)
            .write(false)
            .open(jsonl)?;
        let reader = std::io::BufReader::new(file);

        let mut index = BTreeMap::new();

        for json in reader.lines() {
            let line = serde_json::from_str::<Self>(&json?)?;
            index.insert(VersionRev::with_rev(line.version, line.rev), line.node);
        }

        Ok(index)
    }

    pub async fn read(jsonl: impl AsRef<Path>) -> anyhow::Result<Index> {
        debug!("reading index from {}", jsonl.as_ref().display());

        let file = File::options().read(true).write(false).open(jsonl).await?;
        let mut reader = BufReader::new(file).lines();

        let mut index = BTreeMap::new();

        while let Some(json) = reader.next_line().await? {
            let line = serde_json::from_str::<Self>(&json)?;
            index.insert(VersionRev::with_rev(line.version, line.rev), line.node);
        }

        Ok(index)
    }

    pub async fn write(output: impl AsRef<Path>, id: &Id, index: Index) -> anyhow::Result<()> {
        let output = output.as_ref();

        debug!("writing index for {id} to {}", output.display());

        if let Some(parent) = output.parent() {
            create_dir_all(parent).await?;
        }
        let file = File::create(output).await?;
        let mut writer = BufWriter::new(file);

        let count = index.len();

        for (VersionRev { version, rev }, node) in index {
            let line = IndexLine {
                id: id.clone(),
                version,
                rev,
                node: node,
            };
            let json = serde_json::to_string(&line)?;
            let line = format!("{json}\n");
            writer.write_all(line.as_bytes()).await?;
        }

        writer.flush().await?;

        debug!("wrote {count} index lines to {}", output.display());

        Ok(())
    }
}

/// A "revisioned" version, which is a regular version with an additional revision number.
///
/// This is used to modify package definitions without changing the version number (which should correspond to the upstream),
/// while still allowing package version locking.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VersionRev {
    /// The version number.
    pub version: Version,

    /// The revision number, monotonically increasing and defaults to 0.
    pub rev: u32,
}

impl VersionRev {
    pub fn new(version: Version) -> Self {
        Self { version, rev: 0 }
    }

    pub fn with_rev(version: Version, rev: u32) -> Self {
        Self { version, rev }
    }
}

impl From<VersionRev> for Version {
    fn from(value: VersionRev) -> Self {
        value.version
    }
}

pub struct IndexCache {
    pub map: RwLock<HashMap<Id, Index>>,
}

impl IndexCache {
    pub fn new() -> Self {
        Self {
            map: RwLock::new(HashMap::new()),
        }
    }
}

impl Creeper {
    pub fn blocking_get_reachable_package(
        &self,
        origin: impl IntoIterator<Item = Id>,
    ) -> anyhow::Result<HashSet<Id>> {
        let mut found = origin
            .into_iter()
            .map(|id| (id, false))
            .collect::<HashMap<_, _>>();

        while !found.values().all(|x| *x) {
            for (k, _) in found.clone().into_iter().filter(|(_k, v)| !v) {
                let index = self.blocking_get_index(&k)?;
                let new = index.into_values().flat_map(|node| node.neighbours());

                for id in new {
                    found.entry(id).or_insert(false);
                }

                found.insert(k, true);
            }
        }

        Ok(found.into_keys().collect())
    }

    pub async fn get_index(&self, package: &Id) -> anyhow::Result<Index> {
        if let Some(index) = self.index_cache.map.read().unwrap().get(package) {
            return Ok(index.clone());
        }

        let index = if !package.is_regular() {
            match package.as_str() {
                "vanilla" => self.vanilla.get_index().await?,
                "neoforge" => self.neoforge.get_index().await?,
                "fabric" => self.fabric.get_index().await?,
                "intermediary" => self.intermediary.get_index().await?,
                _ => todo!(),
            }
        } else {
            self.registry.get_index(package).await?
        };

        self.index_cache
            .map
            .write()
            .unwrap()
            .insert(package.clone(), index.clone());

        Ok(index)
    }

    pub async fn get_node(
        &self,
        package: &Id,
        version: &Version,
        rev: u32,
    ) -> anyhow::Result<PackNode> {
        let index = self.get_index(package).await?;
        let node = index
            .get(&VersionRev::with_rev(version.clone(), rev))
            .ok_or(anyhow!("no {version} rev {rev} for {package}"))?;
        Ok(node.clone())
    }

    pub fn blocking_get_index(&self, package: &Id) -> anyhow::Result<Index> {
        if let Some(index) = self.index_cache.map.read().unwrap().get(package) {
            return Ok(index.clone());
        }

        let index = if !package.is_regular() {
            match package.as_str() {
                "vanilla" => self.vanilla.blocking_get_index()?,
                "neoforge" => self.neoforge.blocking_get_index()?,
                "fabric" => self.fabric.blocking_get_index()?,
                "intermediary" => self.intermediary.blocking_get_index()?,
                s => todo!("builtin package {s}"),
            }
        } else {
            self.registry.blocking_get_index(package)?
        };

        self.index_cache
            .map
            .write()
            .unwrap()
            .insert(package.clone(), index.clone());

        Ok(index)
    }

    pub fn blocking_get_node(
        &self,
        package: &Id,
        version: &Version,
        rev: u32,
    ) -> anyhow::Result<PackNode> {
        let index = self.blocking_get_index(package)?;
        let node = index
            .get(&VersionRev::with_rev(version.clone(), rev))
            .ok_or(anyhow!("no {version} rev {rev} for {package}"))?;
        Ok(node.clone())
    }
}
