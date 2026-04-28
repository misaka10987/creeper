use std::{path::PathBuf, str::FromStr};

use anyhow::{anyhow, bail};
use clap::Parser;
use tokio::fs::read_dir;
use tracing::info;

use crate::{
    Id,
    cmd::Execute,
    index::{IndexLine, compile_index},
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

                    let index = compile_index(p.path()).await?;

                    if let Some(output) = &self.output {
                        let output = output.join(id.indexed_path()).with_added_extension("jsonl");
                        IndexLine::write(output, &id, index).await?;
                    }
                }
            }
        }

        info!("index successfully built");

        Ok(())
    }
}
