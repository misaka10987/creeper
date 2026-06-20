use clap::Parser;

use crate::{
    cmd::Execute,
    path::{creeper_cache_dir, creeper_data_dir},
    util::prompt_remove,
};

/// Remove all configurations, user data, and cache.
#[derive(Clone, Debug, Parser)]
pub struct Nuke {
    /// Skip the confirmation prompt.
    #[arg(long, default_value_t = false)]
    confirm: bool,
}

impl Execute for Nuke {
    async fn execute(self, _lib: &crate::Creeper) -> anyhow::Result<()> {
        prompt_remove(creeper_cache_dir()?).await?;
        prompt_remove(creeper_data_dir()?).await?;
        Ok(())
    }
}
