use clap::Parser;
use creeper::{
    Creeper, CreeperConfig,
    cmd::{Execute, run::Run},
};
use stop::stop;
use tokio::runtime;

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
    Run(Run),
}

impl Execute<SubCommand> for Creeper {
    async fn execute(&self, cmd: SubCommand) -> anyhow::Result<()> {
        match cmd {
            SubCommand::Run(run) => self.execute(run).await,
        }
    }
}

fn main() {
    let Args { cfg, cmd } = Args::parse();
    let creeper = Creeper::new(cfg);
    let run = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(stop!());
    run.block_on(creeper.execute(cmd)).unwrap_or_else(stop!());
}
