use anyhow::bail;
use clap::Parser;

use crate::{Creeper, cmd::Execute};

/// Launch the current game instance.
#[derive(Clone, Debug, Parser)]
pub struct Run;

impl Execute for Run {
    async fn execute(lib: &Creeper, _cmd: Self) -> anyhow::Result<()> {
        let inst = lib.inst().await?;
        let mut cmd = inst.launch(lib.inst_dir()?);
        println!("{:?}", cmd);
        let status = cmd.spawn()?.wait()?;
        if !status.success() {
            bail!("minecraft crashed: {status}")
        }
        Ok(())
    }
}
