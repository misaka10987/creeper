mod checksum;
mod cmd;
mod game;
mod id;
mod java;
mod launch;
// mod lock;
mod artifact;
mod index;
mod install;
mod mc;
mod neoforge;
mod pack;
mod path;
mod pbar;
mod prelude;
mod pubgrub;
mod registry;
mod tool;
mod user;
mod util;
mod vanilla;

use clap::Parser;
use reqwest::Client;
use std::{ops::Deref, path::PathBuf, sync::Arc};
use stop::fatal;
use tokio::runtime;
use tracing::{Level, level_filters::LevelFilter};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};
use url::Url;

use crate::{
    artifact::ArtifactManager,
    cmd::{Execute, build_index::BuildIndex, nf_version::NeoForgeVersion, run::Run},
    game::GameManager,
    neoforge::NeoforgeManager,
    path::init_creeper_dirs,
    registry::Registry,
    tool::Tool,
    vanilla::VanillaManager,
};

pub use prelude::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct CreeperInner {
    pub args: CreeperConfig,
    storage: ArtifactManager,
    vanilla: VanillaManager,
    http: Client,
    registry: Registry,
    game: GameManager,
    neoforge: NeoforgeManager,
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
        let http = Client::default();
        let registry = Registry::new(args.registry.clone(), http.clone())?;
        let inst = GameManager::new(args.working_dir.clone());
        let neoforge = NeoforgeManager::new(http.clone());
        let vanilla = VanillaManager::new(http.clone());
        let val = CreeperInner {
            args,
            storage: ArtifactManager::new().await?,
            vanilla,
            http,
            registry,
            neoforge,
            game: inst,
        };
        Ok(Self(Arc::new(val)))
    }

    pub async fn execute(&self, cmd: impl Execute) -> anyhow::Result<()> {
        cmd.execute(self).await
    }

    pub async fn update_all(&self) -> anyhow::Result<()> {
        self.update_registry().await?;
        self.update_vanilla().await?;
        self.update_neoforge().await?;
        Ok(())
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
    /// URL to the package registry.
    ///
    /// Note that only `file://` URLs are supported for now.
    #[arg(long, default_value = "https://creeper-registry.pages.dev/")]
    pub registry: Url,
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
    BuildIndex(BuildIndex),
    #[command(name = "nf-version")]
    NeoForgeVersion(NeoForgeVersion),
    #[clap(hide = true)]
    AwwMan,
}

impl Execute for SubCommand {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        match self {
            SubCommand::Tool(tool) => lib.execute(tool).await,
            SubCommand::Run(run) => lib.execute(run).await,
            SubCommand::AwwMan => Ok(println!("{CREEPER_TEXT_ART}")),
            SubCommand::BuildIndex(build_index) => lib.execute(build_index).await,
            SubCommand::NeoForgeVersion(nf_version) => lib.execute(nf_version).await,
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
