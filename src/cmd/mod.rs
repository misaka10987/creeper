use clap::Parser;

use crate::{Creeper, cmd::run::Run};

mod run;

#[derive(Clone, Debug, Parser)]
pub enum SubCommand {
    Run(Run),
}

pub trait Execute {
    fn execute(
        creeper: &Creeper,
        args: &Self,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}

impl Execute for SubCommand {
    async fn execute(creeper: &Creeper, args: &Self) -> anyhow::Result<()> {
        match args {
            SubCommand::Run(run) => Execute::execute(creeper, run).await,
        }
    }
}
