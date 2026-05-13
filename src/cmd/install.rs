use std::iter::once;

use clap::Parser;
use tokio::fs::{create_dir_all, write};

use crate::cmd::Execute;

/// Install the current game instance as described in `creeper.toml`.
#[derive(Clone, Debug, Parser)]
pub struct Install;

impl Execute for Install {
    async fn execute(self, lib: &crate::Creeper) -> anyhow::Result<()> {
        let package = lib.game.pack().await?;

        lib.update_all().await?;

        let dep = lib.resolve(package.node.dep.clone())?;
        let sorted = lib.sort_dependency(dep)?;

        let mut install = lib.install_all(sorted).await?;
        install.extend(once(package.install.clone()));

        let json = serde_json::to_string(&install)?;

        let path = lib.game.dir().await?.join(".creeper").join("install.json");
        create_dir_all(path.parent().unwrap()).await?;
        write(path, json).await?;

        Ok(())
    }
}
