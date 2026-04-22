use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{anyhow, bail, ensure};
use clap::Parser;
use semver::Version;
use tokio::{
    fs::{File, create_dir_all, read_dir, read_to_string},
    io::{AsyncWriteExt, BufWriter},
};
use tracing::info;

use crate::{
    Id, Package,
    cmd::Execute,
    registry::{IndexLine, VersionRev},
};

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
    async fn execute(self, lib: &crate::Creeper) -> anyhow::Result<()> {
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

                            let toml = read_to_string(&path).await?;
                            let p: Package = toml::from_str(&toml)?;

                            ensure!(p.id == id, "inconsistent id in {}", path.display());
                            ensure!(
                                p.version == version,
                                "inconsistent version in {}",
                                path.display()
                            );
                            ensure!(p.rev == rev, "inconsistent revision in {}", path.display());

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
                        write_index(
                            output.join(id.indexed_path()).with_added_extension("jsonl"),
                            &pack,
                        )
                        .await?;
                    }
                }
            }
        }

        info!("processing neoforge");

        let neoforge = lib
            .get_neoforge_index()
            .await?
            .into_iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    IndexLine {
                        id: Id::neoforge(),
                        version: k.0.clone(),
                        rev: k.1.clone(),
                        node: v.clone(),
                    },
                )
            })
            .collect();

        if let Some(output) = &self.output {
            write_index(
                output
                    .join(Id::neoforge().indexed_path())
                    .with_added_extension("jsonl"),
                &neoforge,
            )
            .await?;
        }

        info!("index successfully built");

        Ok(())
    }
}

pub async fn write_index(
    output: impl AsRef<Path>,
    index: &BTreeMap<VersionRev, IndexLine>,
) -> anyhow::Result<()> {
    let output = output.as_ref();
    create_dir_all(output.parent().unwrap()).await?;
    let file = File::create(output).await?;
    let mut writer = BufWriter::new(file);
    for (_, line) in index {
        let json = serde_json::to_string(line)?;
        let line = format!("{}\n", json);
        writer.write_all(line.as_bytes()).await?;
    }
    writer.flush().await?;
    info!("wrote {} line(s) to {}", index.len(), output.display());
    Ok(())
}
