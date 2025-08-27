use clap::Parser;
use creeper::{Creeper, cmd::Execute, vanilla::VanillaManage};
use semver::Version;

/// Collection of CLI tools basically for development use.
#[derive(Clone, Debug, Parser)]
pub enum Tool {
    LoadInst(LoadInst),
    FetchManifest(FetchManifest),
    FetchMcVersion(FetchMcVersion),
    VanillaInstall(VanillaInstall),
}

impl Execute<Tool> for Creeper {
    async fn execute(&self, cmd: Tool) -> anyhow::Result<()> {
        match cmd {
            Tool::LoadInst(load_inst) => self.execute(load_inst).await,
            Tool::FetchManifest(fetch_manifest) => self.execute(fetch_manifest).await,
            Tool::FetchMcVersion(fetch_mc_version) => self.execute(fetch_mc_version).await,
            Tool::VanillaInstall(vanilla_install) => self.execute(vanilla_install).await,
        }
    }
}

/// Load the configuration for current minecraft instance.
#[derive(Clone, Debug, Parser)]
pub struct LoadInst;

impl Execute<LoadInst> for Creeper {
    async fn execute(&self, _cmd: LoadInst) -> anyhow::Result<()> {
        let inst = self.inst().await?;
        let toml = toml::to_string_pretty(inst)?;
        println!("{toml}");
        Ok(())
    }
}

/// Fetch the minecraft version manifest from online launcher metadata.
#[derive(Clone, Debug, Parser)]
pub struct FetchManifest;

impl Execute<FetchManifest> for Creeper {
    async fn execute(&self, _cmd: FetchManifest) -> anyhow::Result<()> {
        let manifest = self.vanilla_manifest().await?;
        let json = serde_json::to_string_pretty(manifest)?;
        println!("{json}");
        Ok(())
    }
}

/// Fetch the specified minecraft version description file according to the version manifest.
#[derive(Clone, Debug, Parser)]
pub struct FetchMcVersion {
    /// The minecraft version to fetch description for.
    #[arg(value_name = "VERSION")]
    version: Version,
}

impl Execute<FetchMcVersion> for Creeper {
    async fn execute(&self, cmd: FetchMcVersion) -> anyhow::Result<()> {
        let mc_version = self.vanilla_version(cmd.version).await?;
        let json = serde_json::to_string_pretty(&mc_version)?;
        println!("{json}");
        Ok(())
    }
}

/// Install and retrieve metadata of specific minecraft version.
#[derive(Clone, Debug, Parser)]
pub struct VanillaInstall {
    /// The version of installation.
    #[arg(value_name = "VERSION")]
    version: Version,
}

impl Execute<VanillaInstall> for Creeper {
    async fn execute(&self, cmd: VanillaInstall) -> anyhow::Result<()> {
        let install = self.vanilla_install(cmd.version).await?;
        let toml = serde_json::to_string_pretty(&install)?;
        println!("{toml}");
        Ok(())
    }
}
