use crate::{Creeper, cmd::Execute};
use clap::Parser;
use semver::Version;

/// Collection of CLI tools basically for development use.
#[derive(Clone, Debug, Parser)]
pub enum Tool {
    LoadInst(LoadInst),
    FetchManifest(FetchManifest),
    FetchMcVersion(FetchMcVersion),
    VanillaInstall(VanillaInstall),
}

impl Execute for Tool {
    async fn execute(lib: &Creeper, cmd: Self) -> anyhow::Result<()> {
        match cmd {
            Tool::LoadInst(load_inst) => lib.execute(load_inst).await,
            Tool::FetchManifest(fetch_manifest) => lib.execute(fetch_manifest).await,
            Tool::FetchMcVersion(fetch_mc_version) => lib.execute(fetch_mc_version).await,
            Tool::VanillaInstall(vanilla_install) => lib.execute(vanilla_install).await,
        }
    }
}

/// Load the configuration for current minecraft instance.
#[derive(Clone, Debug, Parser)]
pub struct LoadInst;

impl Execute for LoadInst {
    async fn execute(lib: &Creeper, _cmd: Self) -> anyhow::Result<()> {
        let inst = lib.inst().await?;
        let toml = toml::to_string_pretty(inst)?;
        println!("{toml}");
        Ok(())
    }
}

/// Fetch the minecraft version manifest from online launcher metadata.
#[derive(Clone, Debug, Parser)]
pub struct FetchManifest;

impl Execute for FetchManifest {
    async fn execute(lib: &Creeper, _cmd: Self) -> anyhow::Result<()> {
        let manifest = lib.vanilla_manifest().await?;
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

impl Execute for FetchMcVersion {
    async fn execute(lib: &Creeper, cmd: Self) -> anyhow::Result<()> {
        let mc_version = lib.vanilla_version(cmd.version).await?;
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

impl Execute for VanillaInstall {
    async fn execute(lib: &Creeper, cmd: Self) -> anyhow::Result<()> {
        let install = lib.vanilla_install(cmd.version).await?;
        let toml = serde_json::to_string_pretty(&install)?;
        println!("{toml}");
        Ok(())
    }
}
