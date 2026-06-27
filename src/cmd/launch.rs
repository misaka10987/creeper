use clap::Parser;

use crate::{Creeper, cmd::Execute};

/// Launch the current game instance.
#[derive(Clone, Debug, Parser)]
pub struct Launch {
    /// To preview the launch command without executing it.
    #[arg(long, default_value_t = false)]
    pub preview: bool,
}

impl Execute for Launch {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        let mut cmd = lib.launch().await?;

        if self.preview {
            println!("{:?}", cmd.as_std());
            return Ok(());
        }

        let mut proc = cmd.spawn()?;

        proc.wait().await?;

        Ok(())
    }
}
