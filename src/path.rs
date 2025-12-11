use std::path::PathBuf;

use anyhow::anyhow;
use tokio::fs::create_dir_all;
use tracing::debug;

/// The local data storage directory for the app.
pub fn creeper_data_dir() -> anyhow::Result<PathBuf> {
    let dir = dirs::data_local_dir()
        .ok_or(anyhow!("missing local data directory"))?
        .join("creeper");
    Ok(dir)
}

/// The cache directory for the app.
pub fn creeper_cache_dir() -> anyhow::Result<PathBuf> {
    let dir = dirs::cache_dir()
        .ok_or(anyhow!("missing cache directory"))?
        .join("creeper");
    Ok(dir)
}

/// Directory for Minecraft instances managed by Creeper.
pub fn creeper_mc_dir() -> anyhow::Result<PathBuf> {
    let dir = creeper_data_dir()?.join("minecraft");
    Ok(dir)
}

/// Initialize all necessary directories, creating if missing.
pub async fn init_creeper_dirs() -> anyhow::Result<()> {
    debug!("creating creeper directories if missing");
    for dir in [creeper_data_dir()?, creeper_cache_dir()?, creeper_mc_dir()?] {
        create_dir_all(dir).await?;
    }
    Ok(())
}
