use std::path::PathBuf;

use anyhow::bail;

use crate::{
    Id,
    index::{Index, IndexLine},
    path::creeper_cache_dir,
};

pub trait SyncBuiltinIndex {
    fn package(&self) -> Id;
    async fn sync_index(&self) -> anyhow::Result<Index>;
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

        let index = self.sync_index().await?;

        IndexLine::write(cache, &package, index).await?;

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
