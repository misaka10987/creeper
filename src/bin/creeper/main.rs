mod tool;

use clap::Parser;
use creeper::{
    CREEPER_TEXT_ART, Creeper, CreeperConfig,
    cmd::{Execute, run::Run},
};
use stop::stop;
use tokio::runtime;
use tracing::{Level, level_filters::LevelFilter};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::tool::Tool;

/// Minecraft Package Manager.
#[derive(Clone, Debug, Parser)]
struct Args {
    #[clap(flatten)]
    cfg: CreeperConfig,
    /// Set the log filtering level.
    #[arg(name = "loglevel", long, default_value_t = Level::INFO)]
    log_level: Level,
    /// Use verbose output, equivalent to overriding log level to DEBUG.
    #[arg(short, long)]
    verbose: bool,
    /// Use noisy output, equivalent to overriding log level to TRACE.
    #[arg(short, long)]
    noisy: bool,
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
    let Args {
        cfg,
        cmd,
        log_level,
        verbose,
        noisy,
    } = Args::parse();
    let log_level = if noisy {
        Level::TRACE
    } else if verbose {
        Level::DEBUG
    } else {
        log_level
    };
    let layer = IndicatifLayer::new();
    tracing_subscriber::registry()
        .with(LevelFilter::from_level(log_level))
        .with(fmt::layer().with_writer(layer.get_stderr_writer()))
        .with(layer)
        .init();
    let creeper = Creeper::new(cfg);
    let run = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(stop!());
    run.block_on(creeper.execute(cmd)).unwrap_or_else(stop!());
    // drop(span);
}
