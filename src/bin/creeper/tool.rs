use std::ops::Deref;

use clap::Parser;
use creeper::{Creeper, cmd::Execute};

/// Collection of CLI tools basically for development use.
#[derive(Clone, Debug, Parser)]
pub enum Tool {
    LoadInst(LoadInst),
    FetchManifest(FetchManifest),
}

impl Execute<Tool> for Creeper {
    async fn execute(&self, cmd: Tool) -> anyhow::Result<()> {
        match cmd {
            Tool::LoadInst(load_inst) => self.execute(load_inst).await,
            Tool::FetchManifest(fetch_manifest) => self.execute(fetch_manifest).await,
        }
    }
}

/// Load the configuration for current minecraft instance.
#[derive(Clone, Debug, Parser)]
pub struct LoadInst;

impl Execute<LoadInst> for Creeper {
    async fn execute(&self, _cmd: LoadInst) -> anyhow::Result<()> {
        let inst = self.load_inst().await?;
        let toml = toml::to_string_pretty(inst.deref())?;
        println!("{toml}");
        Ok(())
    }
}

/// Fetch the minecraft version manifest from online launcher metadata.
#[derive(Clone, Debug, Parser)]
pub struct FetchManifest;

impl Execute<FetchManifest> for Creeper {
    async fn execute(&self, _cmd: FetchManifest) -> anyhow::Result<()> {
        let manifest = self.fetch_manifest().await?;
        let json = serde_json::to_string_pretty(manifest)?;
        println!("{json}");
        Ok(())
    }
}
