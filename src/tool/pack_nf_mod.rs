use std::collections::{HashMap, HashSet};

use anyhow::bail;
use clap::Parser;
use inquire::{Confirm, Select, Text};
use semver::{Version, VersionReq};
use tracing::{error, warn};
use url::Url;

use crate::{
    Id, Package,
    cmd::Execute,
    neoforge::{NeoforgeMods, neoforge_mods::DependencyType},
    pack::{PackMeta, PackNode},
    util::prompt_valid,
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
            let ids = mods.mods.iter().map(|m| &m.mod_id).collect::<HashSet<_>>();
            if ids.len() < mods.mods.len() {
                bail!("duplicate mod IDs in neoforge.mods.toml")
            }
            let select = Select::new(
                "The JAR file contains multiple mods, select one:",
                ids.into_iter().collect(),
            )
            .prompt()?;
            select.clone()
        };

        let select_mod = mods
            .mods
            .iter()
            .find(|m| m.mod_id == select_mod_id)
            .unwrap();

        let id = match select_mod_id.parse::<Id>() {
            Ok(id) if Confirm::new(&format!("Use {id} as package id?")).prompt()? => id,
            Ok(_) => prompt_valid::<Id>("Enter a custom package id:").await?,
            Err(_) => {
                prompt_valid::<Id>(&format!(
                    "{} is not valid package id, enter one instead:",
                    select_mod_id
                ))
                .await?
            }
        };

        let version = match select_mod.version.parse::<Version>() {
            Ok(v) => v,
            Err(_) => {
                prompt_valid::<Version>(&format!(
                    "{} is not valid semver, enter one instead::",
                    select_mod.version
                ))
                .await?
            }
        };

        let license = match mods.license.parse::<spdx::Expression>() {
            Ok(x) => x,
            Err(_)
                if let Ok(x) =
                    format!("LicenseRef-{}", mods.license).parse::<spdx::Expression>()
                    && Confirm::new(&format!(
                        "{} is not valid SPDX license expression, correct it to LicenseRef-{0}?",
                        mods.license
                    ))
                    .prompt()? =>
            {
                x
            }
            _ => {
                prompt_valid::<spdx::Expression>(&format!(
                    "{} is not valid SPDX license expression, enter one instead:",
                    mods.license
                ))
                .await?
            }
        };

        let name = if let Some(name) = &select_mod.display_name {
            name.clone()
        } else {
            Text::new("Name of the package:").prompt()?
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

        let mut dep = HashMap::new();

        let deps = mods
            .dependencies
            .get(&select_mod_id)
            .cloned()
            .unwrap_or_default();

        for d in deps {
            if d.dependency_type != DependencyType::Required {
                // TODO
                error!(
                    "does not support specifying {} dependency {}",
                    d.dependency_type, d.mod_id
                );
                continue;
            }

            let id = match d.mod_id.parse::<Id>() {
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

            dep.insert(id, req);
        }

        let pack = Package {
            id,
            version,
            rev: 0,
            meta,
            node: PackNode { dep },
            install: Default::default(),
        };

        let toml = toml::to_string(&pack)?;

        println!("{toml}");

        todo!()
    }
}
