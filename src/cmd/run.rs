use anyhow::bail;
use clap::Parser;

use crate::{Creeper, cmd::Execute};

/// Launch the current game instance.
#[derive(Clone, Debug, Parser)]
pub struct Run;

impl Execute<Run> for Creeper {
    async fn execute(&self, _cmd: Run) -> anyhow::Result<()> {
        let inst = self.inst().await?;
        let mut cmd = inst.launch(&inst.dir);
        println!("{:?}", cmd);
        let status = cmd.spawn()?.wait()?;
        if !status.success() {
            bail!("minecraft crashed: {status}")
        }
        Ok(())
    }
}
