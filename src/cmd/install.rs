use std::iter::once;

use clap::Parser;
use tokio::fs::{create_dir_all, read_to_string, write};

use crate::{cmd::Execute, lock::Lock};

/// Install the current game instance as described in `creeper.toml`.
#[derive(Clone, Debug, Parser)]
pub struct Install {
    #[arg(long, default_value_t = false)]
    update: bool,
}

impl Execute for Install {
    async fn execute(self, lib: &crate::Creeper) -> anyhow::Result<()> {
        let package = lib.game.pack().await?;

        let lock_path = lib.game.dir().await?.join("creeper.lock");

        let locked = if !self.update && lock_path.exists() {
            let toml = read_to_string(&lock_path).await?;
            let lock = toml::from_str::<Lock>(&toml)?;

            lock.satisfies(package.node.dep.clone())
                .then_some(lock.package)
        } else {
            None
        };

        let dep = match locked {
            Some(dep) => dep,
            None => {
                lib.update_all().await?;
                let sol = lib.resolve(package.node.dep.clone())?;

                let lock = Lock {
                    registry: lib.args.registry.clone(),
                    package: sol.clone(),
                };
                let toml = toml::to_string(&lock)?;

                write(&lock_path, toml).await?;

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
