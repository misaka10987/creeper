use std::collections::{BTreeSet, HashMap};

use crate::{
    Creeper, Id, YggdrasilClient,
    cmd::Execute,
    index::VersionRev,
    neoforge::{decode_neoforge_version, parse_neoforge_version},
};
use anyhow::{anyhow, ensure};
use clap::Parser;
use colored::Colorize;
use indexmap::IndexMap;
use stop::fatal;

/// Collection of CLI tools basically for development use.
#[derive(Clone, Debug, Parser)]
pub enum Tool {
    LoadInst(LoadInst),
    Resolve(Resolve),
    GetPackage(GetPackage),
    ListVersion(ListVersion),
    GetInstall(GetInstall),
    DiscoverYggdrasil(DiscoverYggdrasil),
    #[command(name = "nf-version")]
    NeoForgeVersion(NeoForgeVersion),
}

impl Execute for Tool {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        match self {
            Tool::LoadInst(load_inst) => lib.execute(load_inst).await,
            Tool::Resolve(resolve) => lib.execute(resolve).await,
            Tool::GetPackage(get_package) => lib.execute(get_package).await,
            Tool::GetInstall(get_install) => lib.execute(get_install).await,
            Tool::DiscoverYggdrasil(discover_yggdrasil) => lib.execute(discover_yggdrasil).await,
            Tool::ListVersion(list_version) => lib.execute(list_version).await,
            Tool::NeoForgeVersion(nf_version) => lib.execute(nf_version).await,
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
    /// The requirements in the `<package>@<version-req>` format.
    #[arg(long)]
    pub req: Vec<String>,

    /// Sort the resolved packages from dependencies to dependents.
    #[arg(long, default_value_t = false)]
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
        let package = lib.query_registry(&id, &version, self.rev).await?;
        let toml = toml::to_string_pretty(&package)?;
        println!("{toml}");
        Ok(())
    }
}

/// Get the installation performed by the specific package.
#[derive(Clone, Debug, Parser)]
pub struct GetInstall {
    /// The package in the `<id>@<version>` format.
    #[arg(value_name = "PACKAGE")]
    pub package: String,
    /// The revision number of this version, defaults to 0.
    #[arg(long, default_value_t = 0)]
    pub rev: u32,
    /// Whether to recursively get the installation of the package's dependencies, defaults to false.
    #[arg(short, long, default_value_t = false)]
    pub recursive: bool,
}

impl Execute for GetInstall {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        let pieces = self.package.split('@').collect::<Vec<_>>();
        ensure!(
            pieces.len() == 2,
            "invalid package version {}, expected <id>@<version>",
            self.package
        );
        let (id, version) = (pieces[0].parse()?, pieces[1].parse()?);

        let install = if self.recursive {
            let package = lib.query_registry(&id, &version, 0).await?;
            lib.recursive_install(package).await?
        } else {
            lib.install(&id, version).await?
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
        let index = lib.get_index(&self.package).await?;

        let versions = index
            .into_keys()
            .map(|VersionRev(version, _rev)| version)
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
