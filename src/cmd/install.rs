use std::iter::once;

use clap::Parser;
use tokio::fs::{create_dir_all, write};
use tracing::info;

use crate::{cmd::Execute, lock::Lock};

/// Install the current game instance as described in `creeper.toml`.
#[derive(Clone, Debug, Parser)]
pub struct Install {
    #[arg(long, default_value_t = false)]
    pub update: bool,
}

impl Execute for Install {
    async fn execute(self, lib: &crate::Creeper) -> anyhow::Result<()> {
        let package = lib.game.pack().await?;

        let lock = lib.game.lock().await?;

        let dep = match lock {
            Some(lock) if lock.satisfies(package.node.dep.clone()) && !self.update => {
                info!("using package lock file");
                lock.package
            }
            _ => {
                info!("ignoring package lock file");

                lib.update_all().await?;
                let sol = lib.resolve(package.node.dep.clone())?;

                let lock = Lock {
                    registry: lib.args.registry.clone(),
                    package: sol.clone(),
                };
                lib.game.set_lock(Some(lock)).await?;

                sol
            }
        };

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
