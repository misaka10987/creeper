use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::bail;
use semver::Version;
use tokio::fs::{create_dir_all, read_to_string, try_exists, write};
use tracing::debug;

use crate::{
    Creeper, Id, Install,
    index::{Index, IndexLine},
    path::creeper_cache_dir,
};

pub trait SyncBuiltinIndex {
    fn package(&self) -> Id;

    async fn sync_index(&self) -> anyhow::Result<Index>;

    fn cache_expiry(&self) -> Duration {
        Duration::from_secs(0)
    }
}

pub trait GetIndex {
    async fn get_index(&self) -> anyhow::Result<Index>;
}

pub trait BlockingGetIndex {
    fn blocking_get_index(&self) -> anyhow::Result<Index>;
}

pub trait UpdateIndex {
    async fn update_index(&self) -> anyhow::Result<()>;
}

impl<T: SyncBuiltinIndex> UpdateIndex for T {
    async fn update_index(&self) -> anyhow::Result<()> {
        let package = self.package();

        let cache = builtin_index_cache(&package)?;

        let last_updated = creeper_cache_dir()?
            .join("builtin")
            .join(&*package)
            .join("index-last-updated");

        if try_exists(&cache).await?
            && self.get_index().await.is_ok()
            && try_exists(&last_updated).await?
        {
            let last_updated = read_to_string(&last_updated).await?.parse::<u64>()?;
            let last_updated = SystemTime::UNIX_EPOCH + Duration::from_secs(last_updated);

            let now = SystemTime::now();

            if let Ok(time) = now.duration_since(last_updated)
                && time < self.cache_expiry()
            {
                debug!("skipping builtin index update for {package} because cache is still valid");
                return Ok(());
            }
        }

        let index = self.sync_index().await?;

        IndexLine::write(cache, &package, index).await?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        create_dir_all(last_updated.parent().unwrap()).await?;

        write(&last_updated, now.to_string()).await?;

        Ok(())
    }
}

fn builtin_index_cache(package: &Id) -> anyhow::Result<PathBuf> {
    let path = creeper_cache_dir()?
        .join("index")
        .join(package.indexed_path())
        .with_added_extension("jsonl");

    Ok(path)
}

impl<T: SyncBuiltinIndex> GetIndex for T {
    async fn get_index(&self) -> anyhow::Result<Index> {
        let package = self.package();

        let cache = builtin_index_cache(&package)?;

        if !cache.exists() {
            bail!("missing package index for {} in local cache", package);
        }

        let index = IndexLine::read(cache).await?;

        Ok(index)
    }
}

impl<T: SyncBuiltinIndex> BlockingGetIndex for T {
    fn blocking_get_index(&self) -> anyhow::Result<Index> {
        let package = self.package();

        let cache = builtin_index_cache(&package)?;

        if !cache.exists() {
            bail!("missing package index for {} in local cache", package);
        }

        let index = IndexLine::blocking_read(cache)?;

        Ok(index)
    }
}

impl Creeper {
    pub(crate) async fn builtin_install(
        &self,
        package: &Id,
        version: &Version,
    ) -> anyhow::Result<Install> {
        if package.is_regular() {
            bail!("{package} is not builtin package");
        }

        let install = match package.as_str() {
            "vanilla" => self.vanilla_install(version).await?,
            "vanilla-server" => self.vanilla_server_install(version).await?,
            "neoforge" => self.neoforge_install(version).await?,
            "neoforge-server" => self.neoforge_server_install(version).await?,
            "fabric" => self.fabric_install(version).await?,
            "intermediary" => self.intermediary_install(version).await?,
            p => todo!("install builtin package {p}"),
        };

        Ok(install)
    }

    pub(crate) async fn update_builtin_index(&self) -> anyhow::Result<()> {
        self.vanilla.update_index().await?;
        self.vanilla_server.update_index().await?;
        self.neoforge.update_index().await?;
        self.neoforge_server.update_index().await?;
        self.fabric.update_index().await?;
        self.intermediary.update_index().await?;

        Ok(())
    }

    pub(crate) async fn get_builtin_index(&self, package: &Id) -> anyhow::Result<Index> {
        if package.is_regular() {
            bail!("{package} is not builtin package");
        }

        let index = match package.as_str() {
            "vanilla" => self.vanilla.get_index().await?,
            "vanilla-server" => self.vanilla_server.get_index().await?,
            "neoforge" => self.neoforge.get_index().await?,
            "neoforge-server" => self.neoforge_server.get_index().await?,
            "fabric" => self.fabric.get_index().await?,
            "intermediary" => self.intermediary.get_index().await?,
            p => todo!("index builtin package {p}"),
        };

        Ok(index)
    }

    pub(crate) fn blocking_get_builtin_index(&self, package: &Id) -> anyhow::Result<Index> {
        if package.is_regular() {
            bail!("{package} is not builtin package");
        }

        let index = match package.as_str() {
            "vanilla" => self.vanilla.blocking_get_index()?,
            "vanilla-server" => self.vanilla_server.blocking_get_index()?,
            "neoforge" => self.neoforge.blocking_get_index()?,
            "neoforge-server" => self.neoforge_server.blocking_get_index()?,
            "fabric" => self.fabric.blocking_get_index()?,
            "intermediary" => self.intermediary.blocking_get_index()?,
            p => todo!("blocking index builtin package {p}"),
        };

        Ok(index)
    }
}
