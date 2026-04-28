use std::collections::HashMap;

use crate::{Creeper, cmd::Execute};
use anyhow::ensure;
use clap::Parser;
use colored::Colorize;
use indexmap::IndexMap;
use semver::Version;
use stop::fatal;

/// Collection of CLI tools basically for development use.
#[derive(Clone, Debug, Parser)]
pub enum Tool {
    LoadInst(LoadInst),
    FetchManifest(FetchManifest),
    FetchMcVersion(FetchMcVersion),
    VanillaInstall(VanillaInstall),
    Resolve(Resolve),
    GetPackage(GetPackage),
    ListNeoforgeVersion(ListNeoforgeVersion),
}

impl Execute for Tool {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        match self {
            Tool::LoadInst(load_inst) => lib.execute(load_inst).await,
            Tool::FetchManifest(fetch_manifest) => lib.execute(fetch_manifest).await,
            Tool::FetchMcVersion(fetch_mc_version) => lib.execute(fetch_mc_version).await,
            Tool::VanillaInstall(vanilla_install) => lib.execute(vanilla_install).await,
            Tool::Resolve(resolve) => lib.execute(resolve).await,
            Tool::GetPackage(get_package) => lib.execute(get_package).await,
            Tool::ListNeoforgeVersion(list_neoforge_version) => {
                lib.execute(list_neoforge_version).await
            }
        }
    }
}

/// Load the configuration for current minecraft instance.
#[derive(Clone, Debug, Parser)]
pub struct LoadInst;

impl Execute for LoadInst {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        let inst = lib.game.pack().await?;
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
pub struct Resolve {
    #[arg(long)]
    pub req: Vec<String>,
    #[arg(long)]
    pub sort: bool,
}

impl Execute for Resolve {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        let mut req = HashMap::new();
        for s in self.req {
            let parts = s.split("@").collect::<Vec<_>>();
            if parts.len() != 2 {
                fatal!(
                    "invalid requirement {}, expected <package>@<version-req>",
                    s
                );
            }
            req.insert(parts[0].parse()?, parts[1].parse()?);
        }

        lib.update_all().await?;

        let sol = match lib.resolve(req) {
            Ok(x) => x,
            Err(e) => {
                fatal!("dependency resolution failed: {}", e);
            }
        };
        eprintln!("{} {} packages", "Resolved".bold().green(), sol.len());

        if self.sort {
            let sorted = lib.sort_dependency(sol)?;
            let mut map = IndexMap::new();
            for (k, v) in sorted {
                map.insert(k, v);
            }
            eprintln!(
                "{} {} packages (from dependencies to dependents)",
                "Sorted".bold().green(),
                map.len()
            );
            println!("{}", toml::to_string_pretty(&map)?);
            return Ok(());
        };

        println!("{}", toml::to_string_pretty(&sol)?);
        Ok(())
    }
}

/// Query the package registry for a specific package version, printing its metadata.
#[derive(Clone, Debug, Parser)]
pub struct GetPackage {
    /// The package in the `<id>@<version>` format.
    #[arg(value_name = "PACKAGE")]
    pub package: String,
    /// The revision number of this version, defaults to 0.
    #[arg(long, default_value_t = 0)]
    pub rev: u32,
}

impl Execute for GetPackage {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        let pieces = self.package.split('@').collect::<Vec<_>>();
        ensure!(
            pieces.len() == 2,
            "invalid package version {}, expected <id>@<version>",
            self.package
        );
        let (id, version) = (pieces[0].parse()?, pieces[1].parse()?);
        let package = lib.get_package(&id, &version, self.rev).await?;
        let toml = toml::to_string_pretty(&package)?;
        println!("{toml}");
        Ok(())
    }
}

#[derive(Clone, Debug, Parser)]
pub struct ListNeoforgeVersion;

impl Execute for ListNeoforgeVersion {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        let versions = lib.list_neoforge_version().await?;
        let json = serde_json::to_string(versions)?;
        println!("{json}");
        Ok(())
    }
}
