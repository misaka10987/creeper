use std::collections::{BTreeMap, HashSet};

use anyhow::bail;
use clap::Parser;
use colored::Colorize;
use inquire::{Select, Text};
use semver::VersionReq;
use tokio::task::spawn_blocking;
use tracing::{error, warn};
use url::Url;

use crate::{
    Id, Install, Package,
    cmd::Execute,
    neoforge::{NeoforgeMods, neoforge_mods::DependencyType},
    pack::{PackMeta, PackNode},
    util::{parse_or_prompt, prompt_correct_license, prompt_save, prompt_valid},
    zip::extract_zip,
};

/// Package a NeoForge mod by downloading form the specified URL and parsing JAR metadata.
#[derive(Clone, Debug, Parser)]
pub struct PackageNeoforgeMod {
    /// Download URL for the NeoForge mod JAR file.
    ///
    /// Note that this URL will be used in the resulting package as download source.
    pub url: Url,
}

impl Execute for PackageNeoforgeMod {
    async fn execute(self, lib: &crate::Creeper) -> anyhow::Result<()> {
        let art = lib
            .download(self.url.to_string(), self.url.to_string(), None, None)
            .await?;

        let jar = lib.retrieve_artifact(&art).await?;

        let toml = extract_zip(jar, "META-INF/neoforge.mods.toml").await?;

        let mods = toml::from_str::<NeoforgeMods>(&toml)?;

        let select_mod_id = if mods.mods.len() == 1 {
            mods.mods[0].mod_id.clone()
        } else {
            let ids = mods
                .mods
                .iter()
                .map(|m| m.mod_id.clone())
                .collect::<HashSet<_>>();

            if ids.len() < mods.mods.len() {
                bail!("duplicate mod IDs in neoforge.mods.toml")
            }

            let select = spawn_blocking(move || {
                Select::new(
                    "The JAR file contains multiple mods, select one:",
                    ids.into_iter().collect(),
                )
                .prompt()
            })
            .await??;

            select.clone()
        };

        let select_mod = mods
            .mods
            .iter()
            .find(|m| m.mod_id == select_mod_id)
            .unwrap();

        let id = parse_or_prompt(&select_mod_id, "package id").await?;

        let version = match select_mod.version.parse() {
            Ok(v) => v,
            Err(_) => parse_or_prompt(&select_mod.version, "semver").await?,
        };

        let license = prompt_correct_license(&mods.license).await?;

        let name = if let Some(name) = &select_mod.display_name {
            name.clone()
        } else {
            spawn_blocking(|| Text::new("Name of the package:").prompt()).await??
        };

        let authors = select_mod
            .authors
            .as_ref()
            .map_or(vec![], |s| s.split(", ").map(ToString::to_string).collect());

        let meta = PackMeta {
            name,
            authors,
            desc: select_mod.description.clone(),
            license: Some(license),
        };

        let mut dep = BTreeMap::new();

        let deps = mods
            .dependencies
            .get(&select_mod_id)
            .cloned()
            .unwrap_or_default();

        let mut conflict = BTreeMap::new();

        for d in deps {
            let id = match d.mod_id.parse::<Id>() {
                Ok(id) if id == Id::minecraft() => Id::vanilla(),
                Ok(id) => id,
                Err(_) => {
                    prompt_valid::<Id>(&format!(
                        "dependency {} is not valid package id, enter one instead:",
                        d.mod_id
                    ))
                    .await?
                }
            };

            let req = if let Some(rng) = d.version_range {
                VersionReq::try_from(rng)?
            } else {
                warn!("dependency {id} does not specify version range, defaulting to *");
                VersionReq::STAR
            };

            match d.ordering {
                crate::neoforge::neoforge_mods::Ordering::After
                | crate::neoforge::neoforge_mods::Ordering::None => {}
                ord => {
                    error!("ignoring unsupported {ord} ordering for dependency {id}");
                }
            }

            match d.dependency_type {
                DependencyType::Required => {
                    dep.insert(id, req);
                }
                DependencyType::Incompatible => {
                    conflict.insert(id, req);
                }
                t => {
                    error!("does not support specifying {t} dependency {id}, skipping");
                    continue;
                }
            };
        }

        let pack = Package {
            id,
            version,
            rev: 0,
            meta,
            node: PackNode {
                dep,
                conflict,
                ..Default::default()
            },
            install: Install {
                mc_mod: vec![art],
                ..Default::default()
            },
        };

        let toml = toml::to_string(&pack)?;

        eprintln!("{} {}@{}", "Packaged".bold().green(), pack.id, pack.version);

        println!("{toml}");

        let path = pack
            .id
            .indexed_path()
            .as_ref()
            .join(pack.version.to_string())
            .join("0.toml");

        prompt_save(toml, path).await?;

        Ok(())
    }
}
