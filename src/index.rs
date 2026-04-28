use std::{collections::BTreeMap, io::BufRead, path::Path, str::FromStr};

use anyhow::{anyhow, bail, ensure};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use tokio::{
    fs::{File, create_dir_all, read_dir, read_to_string},
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter},
};
use tracing::debug;

use crate::{Creeper, Id, Package, pack::PackNode};

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

            index.insert(VersionRev(version.clone(), rev), pack.node);
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
            index.insert(VersionRev(line.version, line.rev), line.node);
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
            index.insert(VersionRev(line.version, line.rev), line.node);
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

        for (version, node) in index {
            let line = IndexLine {
                id: id.clone(),
                version: version.0,
                rev: version.1,
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

#[derive(Clone, PartialEq, Eq)]
pub struct VersionRev(pub Version, pub u32);
impl PartialOrd for VersionRev {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.0.cmp(&other.0).then(self.1.cmp(&other.1)))
    }
}
impl Ord for VersionRev {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl Creeper {
    pub fn blocking_get_index(&self, package: &Id) -> anyhow::Result<Index> {
        if !package.is_regular() {
            match package.as_str() {
                "vanilla" => return self.vanilla.blocking_get_index().cloned(),
                "neoforge" => return self.neoforge.blocking_get_index().cloned(),
                _ => todo!(),
            }
        }
        self.registry.blocking_get_index(package)
    }

    pub fn blocking_get_node(
        &self,
        package: &Id,
        version: &Version,
        rev: u32,
    ) -> anyhow::Result<PackNode> {
        let index = self.blocking_get_index(package)?;
        let node = index
            .get(&VersionRev(version.clone(), rev))
            .ok_or(anyhow!("no {version} rev {rev} for {package}"))?;
        Ok(node.clone())
    }
}
