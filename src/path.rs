use std::{env::temp_dir, path::PathBuf};

use anyhow::anyhow;
use tokio::fs::create_dir_all;
use tracing::debug;
use whoami::username_os;

pub fn creeper_config_dir() -> anyhow::Result<PathBuf> {
    let dir = dirs::config_dir()
        .ok_or(anyhow!("missing config directory"))?
        .join("creeper");
    Ok(dir)
}

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

// pub fn creeper_state_dir() -> anyhow::Result<PathBuf> {
//     let dir = dirs::state_dir()
//         .or_else(|| {
//             debug!("state directory not configured, falling back to cache directory");
//             dirs::cache_dir()
//         })
//         .ok_or(anyhow!("missing state directory"))?
//         .join("creeper");

//     Ok(dir)
// }

// pub fn creeper_log_dir() -> anyhow::Result<PathBuf> {
//     let dir = creeper_state_dir()?.join("log");

//     Ok(dir)
// }

pub fn creeper_tmp_dir() -> anyhow::Result<PathBuf> {
    let dir = temp_dir().join("creeper").join(username_os()?);

    Ok(dir)
}

/// Initialize all necessary directories, creating if missing.
pub async fn init_creeper_dirs() -> anyhow::Result<()> {
    debug!("creating creeper directories if missing");
    for dir in [
        creeper_config_dir()?,
        creeper_data_dir()?,
        creeper_cache_dir()?,
        creeper_mc_dir()?,
    ] {
        create_dir_all(dir).await?;
    }
    Ok(())
}
