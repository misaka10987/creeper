use std::{collections::BTreeMap, path::PathBuf, str::FromStr};

use anyhow::{anyhow, bail};
use clap::Parser;
use semver::Version;
use tokio::{
    fs::{File, create_dir_all, read_dir, read_to_string},
    io::{AsyncWriteExt, BufWriter},
};
use tracing::info;

use crate::{Id, Package, cmd::Execute, registry::IndexLine};

/// Build the lookup index for a package registry.
#[derive(Clone, Debug, Parser)]
#[clap(rename_all = "kebab-case")]
pub struct BuildIndex {
    /// Package registry directory.
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    /// Output directory to write the index to.
    ///
    /// If not specified, will only perform a validation on the input.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

impl Execute for BuildIndex {
    async fn execute(self, _lib: &crate::Creeper) -> anyhow::Result<()> {
        let mut read = read_dir(&self.input).await?;
        while let Some(lv1) = read.next_entry().await? {
            let lv1_name = lv1.file_name();
            let lv1_name = lv1_name
                .to_str()
                .and_then(|s| Id::is_valid_index_lv1(s).then_some(s))
                .ok_or(anyhow!("invalid index: {}", lv1.path().display()))?;
            let lv1_path = lv1.path();
            let mut read = read_dir(&lv1_path).await?;
            while let Some(lv2) = read.next_entry().await? {
                let lv2_name = lv2.file_name();
                let lv2_name = lv2_name
                    .to_str()
                    .and_then(|s| Id::is_valid_index_lv2(s).then_some(s))
                    .ok_or(anyhow!("invalid index: {}", lv2.path().display()))?;
                let lv2_path = lv2.path();
                let mut read = read_dir(&lv2_path).await?;
                while let Some(p) = read.next_entry().await? {
                    let id = p
                        .file_name()
                        .to_str()
                        .ok_or(anyhow!("invalid name: {}", p.path().display()))
                        .and_then(Id::from_str)?;
                    if !id.is_of_index(lv1_name, lv2_name) {
                        bail!("index and name mismatch: {}", p.path().display());
                    }

                    info!("processing package {}", id);

                    let mut pack = BTreeMap::new();

                    let mut read = read_dir(p.path()).await?;
                    while let Some(v) = read.next_entry().await? {
                        let version = v.file_name();
                        let version = version
                            .to_str()
                            .ok_or(anyhow!("invalid name {}", v.path().display()))?;
                        let version = Version::parse(version)?;

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
                                .ok_or(anyhow!("failed to parse filename: {}", path.display()))?;
                            let rev: u32 = rev.parse()?;

                            let version_rev = VersionRev(version.clone(), rev);

                            let toml = read_to_string(path).await?;
                            let p: Package = toml::from_str(&toml)?;
                            let node = p.node;

                            let line = IndexLine {
                                id: id.clone(),
                                version: version.clone(),
                                rev,
                                node,
                            };

                            pack.insert(version_rev, line);
                        }
                    }

                    if let Some(output) = &self.output {
                        let file = output.join(id.indexed_path()).with_added_extension("jsonl");
                        create_dir_all(file.parent().unwrap()).await?;
                        let file = File::create(file).await?;
                        let mut writer = BufWriter::new(file);
                        for (_, line) in &pack {
                            let json = serde_json::to_string(line)?;
                            let line = format!("{}\n", json);
                            writer.write_all(line.as_bytes()).await?;
                        }
                        writer.flush().await?;
                        info!("wrote {} line(s) to index for {id}", pack.len());
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq)]
struct VersionRev(Version, u32);
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
