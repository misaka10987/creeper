use anyhow::{anyhow, bail};
use clap::Parser;
use colored::Colorize;
use std::path::PathBuf;
use tokio::fs::{create_dir_all, try_exists, write};

use crate::{Id, Package, cmd::Execute, pack::PackMeta};

/// Create a new creeper package in an existing directory.
#[derive(Clone, Debug, Parser)]
pub struct Init {
    #[arg(value_name = "PATH", default_value = ".")]
    pub path: PathBuf,
    /// Set the resulting package name, defaults to the directory name.
    pub name: Option<String>,
}

impl Execute for Init {
    async fn execute(self, _lib: &crate::Creeper) -> anyhow::Result<()> {
        create_dir_all(&self.path).await?;

        let path = self.path.canonicalize()?;

        let name = self.name.unwrap_or(
            path.file_name()
                .ok_or(anyhow!("cannot retrieve directory name"))?
                .display()
                .to_string(),
        );

        let id = name
            .to_ascii_lowercase()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .parse::<Id>()?;

        let package = Package {
            id: id.clone(),
            version: "0.1.0".parse().unwrap(),
            rev: 0,
            node: Default::default(),
            meta: PackMeta {
                name,
                authors: vec![],
                desc: "".into(),
                license: None,
            },
            install: Default::default(),
        };

        let toml = path.join("creeper.toml");

        if try_exists(&toml).await? {
            bail!(
                "cannot initialize on existing creeper package {}",
                path.display()
            );
        }

        write(&toml, toml::to_string_pretty(&package)?).await?;

        eprintln!(
            "{} creeper package {}",
            "Initialized".bold().green(),
            path.display()
        );

        Ok(())
    }
}
