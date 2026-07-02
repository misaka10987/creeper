use anyhow::anyhow;
use clap::Parser;
use colored::Colorize;
use tracing::{error, info};
use url::Url;

use crate::{
    Id, Install, Package,
    cmd::Execute,
    fabric::FabricMod,
    pack::{PackMeta, PackNode},
    path::creeper_cache_dir,
    util::{parse_or_prompt, prompt_save},
    zip::{extract_zip, extract_zip_to},
};

#[derive(Clone, Debug, Parser)]
pub struct PackageFabricMod {
    pub url: Url,
}

impl Execute for PackageFabricMod {
    async fn execute(self, lib: &crate::Creeper) -> anyhow::Result<()> {
        let art = lib
            .download(self.url.to_string(), self.url.to_string(), None, None)
            .await?;

        let jar = lib.retrieve_artifact(&art).await?;

        let json = extract_zip(&jar, "fabric.mod.json").await?;

        let metadata = serde_json::from_str::<FabricMod>(&json)?;

        let id = parse_or_prompt::<Id>(&metadata.id, "package id").await?;

        let license = match metadata.license {
            Some(x) if let Ok(x) = x.parse() => Some(x),
            Some(x) if let Ok(x) = format!("LicenseRef-{x}").parse() => Some(x),
            Some(x) => Some(parse_or_prompt(&x, "SPDX license expression").await?),
            None => None,
        };

        let mut node = PackNode::default();

        for (id, dep) in metadata.depends {
            let id = match id.parse() {
                Ok(id) => id,
                Err(_) => parse_or_prompt(&id, "package id").await?,
            };

            let req = dep.prompt_normalize().await?;

            node.dep.insert(id, req);
        }

        for (id, dep) in metadata.breaks {
            let id = match id.parse() {
                Ok(id) => id,
                Err(_) => parse_or_prompt(&id, "package id").await?,
            };

            let req = dep.prompt_normalize().await?;

            node.conflict.insert(id, req);
        }

        if !metadata.recommends.is_empty() {
            error!("does not support recommended dependencies in fabric.mod.json");
        }

        if !metadata.suggests.is_empty() {
            error!("does not support suggested dependencies in fabric.mod.json");
        }

        if !metadata.conflicts.is_empty() {
            error!("does not support conflict dependencies in fabric.mod.json");
        }

        if !metadata.provides.is_empty() {
            error!("does not support provided dependencies in fabric.mod.json");
        }

        let extract = creeper_cache_dir()?.join("extract");

        for jij in metadata.jars.into_iter().map(|x| x.file) {
            let file = jij.file_name().ok_or(anyhow!("invalid filename"))?;

            let path = extract.join(file);

            extract_zip_to(&jar, jij, &path).await?;

            if let Ok(json) = extract_zip(&path, "fabric.mod.json").await
                && let Ok(metadata) = serde_json::from_str::<FabricMod>(&json)
                && let Ok(id) = metadata.id.parse()
                && let Some(req) = node.dep.get(&id)
                && req.matches(&metadata.version)
            {
                info!(
                    "{id}@{} is packaged jar-in-jar, skipping from dependencies",
                    metadata.version
                );
                node.dep.remove(&id);
            }
        }

        let pack = Package {
            id,
            version: metadata.version,
            rev: 0,
            node,
            meta: PackMeta {
                name: metadata.name.unwrap_or(metadata.id),
                authors: metadata.authors.into_iter().map(|x| x.name()).collect(),
                desc: metadata.description,
                license,
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
