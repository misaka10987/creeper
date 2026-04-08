use crate::{Creeper, cmd::Execute};
use clap::Parser;
use colored::Colorize;
use inquire::Editor;
use semver::Version;
use stop::fatal;

/// Collection of CLI tools basically for development use.
#[derive(Clone, Debug, Parser)]
pub enum Tool {
    LoadInst(LoadInst),
    FetchManifest(FetchManifest),
    FetchMcVersion(FetchMcVersion),
    VanillaInstall(VanillaInstall),
    InteractiveResolve(InteractiveResolve),
}

impl Execute for Tool {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        match self {
            Tool::LoadInst(load_inst) => lib.execute(load_inst).await,
            Tool::FetchManifest(fetch_manifest) => lib.execute(fetch_manifest).await,
            Tool::FetchMcVersion(fetch_mc_version) => lib.execute(fetch_mc_version).await,
            Tool::VanillaInstall(vanilla_install) => lib.execute(vanilla_install).await,
            Tool::InteractiveResolve(interactive_resolve) => lib.execute(interactive_resolve).await,
        }
    }
}

/// Load the configuration for current minecraft instance.
#[derive(Clone, Debug, Parser)]
pub struct LoadInst;

impl Execute for LoadInst {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
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
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
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
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        let mc_version = lib.vanilla_version(self.version).await?;
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
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        let install = lib.vanilla_install(self.version).await?;
        let toml = serde_json::to_string_pretty(&install)?;
        println!("{toml}");
        Ok(())
    }
}

#[derive(Clone, Debug, Parser)]
pub struct InteractiveResolve;

impl Execute for InteractiveResolve {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        let toml = Editor::new(&format!(
            "Input dependencies in TOML format, e.g. {}",
            "minecraft = \"1.16.5\"".bold()
        ))
        .prompt()?;
        let dep = toml::from_str(&toml)?;
        let sol = match lib.registry.resolve(dep) {
            Ok(x) => x,
            Err(e) => {
                fatal!("Dependency resolution failed: {}", e);
            }
        };
        eprintln!("Resolved {} packages:", sol.len());
        println!("{}", toml::to_string_pretty(&sol)?);
        Ok(())
    }
}
