mod build_index;
mod download;
mod pack_fabric_mod;
mod pack_nf_mod;

mod prelude;

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    Creeper, Id, YggdrasilClient,
    cmd::Execute,
    id::{IdVersion, IdVersionReq},
    neoforge::{decode_neoforge_version, parse_neoforge_version},
};
use anyhow::{anyhow, bail};
use clap::Parser;
use colored::Colorize;
use indexmap::IndexMap;
use stop::fatal;

pub use prelude::*;

/// Collection of CLI tools.
#[derive(Clone, Debug, Parser)]
pub enum Tool {
    BuildIndex(BuildIndex),

    LoadInst(LoadInst),

    Resolve(Resolve),

    GetPackage(GetPackage),

    ListVersion(ListVersion),

    GetInstall(GetInstall),

    DiscoverYggdrasil(DiscoverYggdrasil),

    #[command(name = "nf-version")]
    NeoForgeVersion(NeoForgeVersion),

    #[command(name = "pack-nf-mod")]
    PackageNeoforgeMod(PackageNeoforgeMod),

    Download(Download),

    #[command(name = "pack-fabric-mod")]
    PackageFabricMod(PackageFabricMod),
}

impl Execute for Tool {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        match self {
            Tool::BuildIndex(build_index) => lib.execute(build_index).await,
            Tool::LoadInst(load_inst) => lib.execute(load_inst).await,
            Tool::Resolve(resolve) => lib.execute(resolve).await,
            Tool::GetPackage(get_package) => lib.execute(get_package).await,
            Tool::GetInstall(get_install) => lib.execute(get_install).await,
            Tool::DiscoverYggdrasil(discover_yggdrasil) => lib.execute(discover_yggdrasil).await,
            Tool::ListVersion(list_version) => lib.execute(list_version).await,
            Tool::NeoForgeVersion(nf_version) => lib.execute(nf_version).await,
            Tool::PackageNeoforgeMod(package_neoforge_mod) => {
                lib.execute(package_neoforge_mod).await
            }
            Tool::Download(download) => lib.execute(download).await,
            Tool::PackageFabricMod(package_fabric_mod) => lib.execute(package_fabric_mod).await,
        }
    }
}

/// Load the configuration for current minecraft instance.
#[derive(Clone, Debug, Parser)]
pub struct LoadInst;

impl Execute for LoadInst {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        let inst = lib.game.pack().await?;
        let toml = toml::to_string_pretty(&inst)?;
        println!("{toml}");
        Ok(())
    }
}

/// Resolve the dependencies of a set of requirements.
#[derive(Clone, Debug, Parser)]
pub struct Resolve {
    /// The requirements.
    #[arg(long, value_name = "<PACKAGE>[@<VERSION_REQ>]")]
    pub req: Vec<IdVersionReq>,

    /// Sort the resolved packages from dependencies to dependents.
    #[arg(long, default_value_t = false)]
    pub sort: bool,
}

impl Execute for Resolve {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        let req = self
            .req
            .into_iter()
            .map(|x| (x.id, x.version_req))
            .collect::<BTreeMap<_, _>>();

        if req.len() == 0 {
            bail!("nothing to resolve, please specify at least one requirement");
        }

        lib.update().await?;

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
    /// The specified package and version.
    #[arg(value_name = "<PACKAGE>@<VERSION>")]
    pub package: IdVersion,

    /// The revision number of this version, defaults to 0.
    #[arg(long, default_value_t = 0)]
    pub rev: u32,
}

impl Execute for GetPackage {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        let package = lib
            .query_registry(&self.package.id, &self.package.version, self.rev)
            .await?;
        let toml = toml::to_string_pretty(&package)?;
        println!("{toml}");
        Ok(())
    }
}

/// Get the installation performed by the specific package.
#[derive(Clone, Debug, Parser)]
pub struct GetInstall {
    /// The specified package and version.
    #[arg(value_name = "<PACKAGE>@<VERSION>")]
    pub package: IdVersion,

    /// The revision number of this version, defaults to 0.
    #[arg(long, default_value_t = 0)]
    pub rev: u32,

    /// Whether to recursively get the installation of the package's dependencies, defaults to false.
    #[arg(short, long, default_value_t = false)]
    pub recursive: bool,
}

impl Execute for GetInstall {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        let install = if self.recursive {
            let package = lib
                .query_registry(&self.package.id, &self.package.version, 0)
                .await?;
            lib.recursive_install(package).await?
        } else {
            lib.install(&self.package.id, &self.package.version, self.rev)
                .await?
        };

        let json = serde_json::to_string(&install)?;
        println!("{json}");
        Ok(())
    }
}

/// List all versions available for the specified package.
#[derive(Clone, Debug, Parser)]
pub struct ListVersion {
    /// The package.
    #[arg(value_name = "PACKAGE")]
    pub package: Id,
}

impl Execute for ListVersion {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        lib.update().await?;

        let index = lib.get_index(&self.package).await?;

        let versions = index
            .into_keys()
            .map(|v| v.version)
            .collect::<BTreeSet<_>>();

        let json = serde_json::to_string(&versions)?;
        println!("{json}");
        Ok(())
    }
}

/// Discover a Yggdrasil service at the game server, following the authlib API Location Indication (ALI).
#[derive(Clone, Debug, Parser)]
pub struct DiscoverYggdrasil {
    /// The server to connect. Can be either a hostname or a URL.
    #[arg(value_name = "SERVER")]
    pub server: String,
}

impl Execute for DiscoverYggdrasil {
    async fn execute(self, _lib: &Creeper) -> anyhow::Result<()> {
        let client = YggdrasilClient::new(self.server, "".into(), Default::default())?;

        let url = client.api().await?;

        println!("{url}");

        Ok(())
    }
}

/// Convert between NeoForge version numbers and semver.
#[derive(Clone, Debug, Parser)]
pub struct NeoForgeVersion {
    /// The version to convert in either NeoForge or semver format.
    #[arg(value_name = "VERSION")]
    version: String,
    /// To encode a NeoForge version to semver, otherwise decode.
    #[arg(long, default_value_t = false)]
    encode: bool,
}

impl Execute for NeoForgeVersion {
    async fn execute(self, _lib: &crate::Creeper) -> anyhow::Result<()> {
        if self.encode {
            let encoded = parse_neoforge_version(&self.version)
                .ok_or(anyhow!("unable to encode {} as semver", self.version))?;
            println!("{encoded}");
            return Ok(());
        }
        let decoded = decode_neoforge_version(&self.version.parse()?);
        println!("{decoded}");
        Ok(())
    }
}
