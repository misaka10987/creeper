use anyhow::bail;
use clap::Parser;

use crate::cmd::Execute;

/// Launch the current game instance.
#[derive(Clone, Debug, Parser)]
pub struct Run {}

impl Execute for Run {
    async fn execute(creeper: &crate::Creeper, _args: &Self) -> anyhow::Result<()> {
        let inst = creeper.req_inst().await;
        let mut cmd = inst.launch(&inst.dir);
        println!("{:?}", cmd);
        let status = cmd.spawn()?.wait()?;
        if !status.success() {
            bail!("minecraft crashed: {status}")
        }
        Ok(())
    }
}
