use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::bail;
use tokio::fs::{create_dir_all, read_to_string, try_exists, write};
use tracing::debug;

use crate::{
    Id,
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
