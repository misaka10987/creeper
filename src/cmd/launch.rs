use clap::Parser;

use crate::{Creeper, cmd::Execute};

/// Launch the current game instance.
#[derive(Clone, Debug, Parser)]
pub struct Launch;

impl Execute for Launch {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        let mut cmd = lib.launch().await?;

        let mut proc = cmd.spawn()?;

        proc.wait().await?;

        Ok(())
    }
}
