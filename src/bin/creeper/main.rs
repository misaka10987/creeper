mod tool;

use std::io::stderr;

use clap::Parser;
use creeper::{
    CREEPER_TEXT_ART, Creeper, CreeperConfig,
    cmd::{Execute, run::Run},
};
use stop::stop;
use tokio::runtime;

use crate::tool::Tool;

/// Minecraft Package Manager.
#[derive(Clone, Debug, Parser)]
struct Args {
    #[clap(flatten)]
    cfg: CreeperConfig,
    #[command(subcommand)]
    cmd: SubCommand,
}

#[derive(Clone, Debug, Parser)]
enum SubCommand {
    #[command(subcommand)]
    Tool(Tool),
    Run(Run),
    #[clap(hide = true)]
    AwwMan,
}

impl Execute<SubCommand> for Creeper {
    async fn execute(&self, cmd: SubCommand) -> anyhow::Result<()> {
        match cmd {
            SubCommand::Tool(tool) => self.execute(tool).await,
            SubCommand::Run(run) => self.execute(run).await,
            SubCommand::AwwMan => Ok(println!("{CREEPER_TEXT_ART}")),
        }
    }
}

fn main() {
    let Args { cfg, cmd } = Args::parse();
    tracing_subscriber::fmt().with_writer(stderr).init();
    let creeper = Creeper::new(cfg);
    let run = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(stop!());
    run.block_on(creeper.execute(cmd)).unwrap_or_else(stop!());
}
