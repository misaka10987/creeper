mod checksum;
mod cmd;
mod http;
mod id;
mod inst;
mod java;
mod launch;
mod lock;
mod mc;
mod pack;
mod path;
mod pbar;
mod prelude;
mod registry;
mod storage;
mod tool;
mod user;
mod util;
mod vanilla;

use anyhow::anyhow;
use clap::Parser;
use reqwest::Client;
use std::{
    env::current_dir,
    ops::Deref,
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use stop::fatal;
use tokio::runtime;
use tracing::{Level, level_filters::LevelFilter};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    cmd::{Execute, run::Run},
    path::init_creeper_dirs,
    storage::StorageManager,
    tool::Tool,
    vanilla::VanillaManager,
};

pub use prelude::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct CreeperInner {
    pub args: CreeperConfig,
    storage: StorageManager,
    vanilla: VanillaManager,
    http: Client,
    inst_dir: OnceLock<PathBuf>,
    inst: OnceLock<Inst>,
}

#[derive(Clone)]
pub struct Creeper(Arc<CreeperInner>);

impl Deref for Creeper {
    type Target = CreeperInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Creeper {
    pub async fn new(args: CreeperConfig) -> anyhow::Result<Self> {
        init_creeper_dirs().await?;
        let val = CreeperInner {
            args,
            storage: StorageManager::new().await?,
            vanilla: VanillaManager::new(),
            http: Default::default(),
            inst_dir: OnceLock::new(),
            inst: OnceLock::new(),
        };
        Ok(Self(Arc::new(val)))
    }

    pub fn working_dir(&self) -> anyhow::Result<PathBuf> {
        Ok(self.args.working_dir.clone().unwrap_or(current_dir()?))
    }

    pub fn inst_dir(&self) -> anyhow::Result<&PathBuf> {
        if let Some(dir) = self.inst_dir.get() {
            return Ok(dir);
        }
        let wd = self.working_dir()?;
        let found = Inst::find_dir(wd).ok_or(anyhow!("not in any game instance"))?;
        Ok(self.inst_dir.get_or_init(|| found))
    }

    pub async fn inst(&self) -> anyhow::Result<&Inst> {
        if let Some(inst) = self.inst.get() {
            return Ok(inst);
        }
        let inst = Inst::load(self.inst_dir()?).await?;
        Ok(self.inst.get_or_init(|| inst))
    }

    pub async fn execute(&self, cmd: impl Execute) -> anyhow::Result<()> {
        Execute::execute(self, cmd).await
    }
}

#[derive(Clone, Debug, Parser)]
#[command(version)]
pub struct CreeperConfig {
    /// Rewrite the home directory for current minecraft instance.
    ///
    /// If not specified, would recursively look up parent directory from current directory until a `creeper.toml` is found.
    #[arg(name = "dir", short, long)]
    pub working_dir: Option<PathBuf>,
}

pub const CREEPER_TEXT_ART: &str = r#"
🟩🟩🟩⬜⬜🟩🟩🟩
🟩🟩🟩🟩🟩🟩🟩⬜
🟩⬛⬛🟩🟩⬛⬛⬜
🟩⬛⬛🟩🟩⬛⬛🟩
🟩🟩🟩⬛⬛⬜🟩🟩
🟩🟩⬛⬛⬛⬛🟩⬜
⬜🟩⬛⬛⬛⬛🟩🟩
🟩🟩⬛🟩🟩⬛🟩🟩
"#;

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

impl Execute for SubCommand {
    async fn execute(lib: &Creeper, cmd: SubCommand) -> anyhow::Result<()> {
        match cmd {
            SubCommand::Tool(tool) => lib.execute(tool).await,
            SubCommand::Run(run) => lib.execute(run).await,
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
    let run = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(fatal!());
    let creeper = run.block_on(Creeper::new(cfg)).unwrap_or_else(fatal!());
    run.block_on(creeper.execute(cmd)).unwrap_or_else(fatal!());
}
