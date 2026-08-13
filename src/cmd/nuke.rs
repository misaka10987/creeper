use clap::Parser;

use crate::{
    cmd::Execute,
    path::{creeper_cache_dir, creeper_data_dir},
};

/// Remove all configurations, user data, and cache.
#[derive(Clone, Debug, Parser)]
pub struct Nuke {
    /// Skip the confirmation prompt.
    #[arg(long, default_value_t = false)]
    pub confirm: bool,
}

impl Execute for Nuke {
    async fn execute(self, lib: &crate::Creeper) -> anyhow::Result<()> {
        lib.prompt_remove(creeper_cache_dir()?).await?;
        lib.prompt_remove(creeper_data_dir()?).await?;
        Ok(())
    }
}
