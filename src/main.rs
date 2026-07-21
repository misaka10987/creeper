mod artifact;
mod asset;
mod builtin;
mod checksum;
mod cmd;
mod dev;
mod fabric;
mod game;
mod id;
mod index;
mod install;
mod jar;
mod launch;
mod lock;
mod ms;
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
mod yggdrasil;
mod zip;

use clap::Parser;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use std::{
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
};
use stop::fatal;
use tokio::{
    fs::{read_to_string, write},
    runtime,
};
use tracing::{Level, info, level_filters::LevelFilter};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};
use url::Url;

use crate::{
    artifact::ArtifactManager,
    cmd::Execute,
    dev::Dev,
    fabric::{FabricManager, IntermediaryManager},
    game::GameManager,
    index::IndexCache,
    neoforge::{NeoforgeManager, NeoforgeServerManager},
    path::{creeper_config_dir, init_creeper_dirs},
    registry::Registry,
    tool::Tool,
    user::UserManager,
    vanilla::{VanillaManager, VanillaServerManager},
};

pub use prelude::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct CreeperInner {
    pub args: Args,
    pub config: Config,
    artifact: ArtifactManager,
    vanilla: VanillaManager,
    vanilla_server: VanillaServerManager,
    http: Client,
    registry: Registry,
    index_cache: IndexCache,
    game: GameManager,
    neoforge: NeoforgeManager,
    neoforge_server: NeoforgeServerManager,
    fabric: FabricManager,
    intermediary: IntermediaryManager,
    user: UserManager,
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
    async fn load_config(path: impl AsRef<Path>) -> anyhow::Result<Config> {
        let path = path.as_ref();

        if !path.exists() {
            info!("no config file at {}, using default", path.display());

            let config = Config::default();

            let toml = toml::to_string_pretty(&config)?;

            write(path, toml).await?;

            return Ok(config);
        }

        let toml = read_to_string(path).await?;

        let config = toml::from_str(&toml)?;

        Ok(config)
    }

    pub async fn new(args: Args) -> anyhow::Result<Self> {
        init_creeper_dirs().await?;

        let path = args
            .config
            .clone()
            .unwrap_or(creeper_config_dir()?.join("config.toml"));

        let config = Self::load_config(path).await?;

        let http = Client::default();
        let registry = Registry::new(config.registry.clone(), http.clone())?;
        let game = GameManager::new(args.dir.clone());
        let neoforge = NeoforgeManager::new(http.clone());
        let vanilla = VanillaManager::new(http.clone());
        let artifact = ArtifactManager::new(http.clone(), args.offline).await?;
        let user = UserManager::new();
        let fabric = FabricManager::new(http.clone());
        let intermediary = IntermediaryManager::new(http.clone());
        let vanilla_server = VanillaServerManager::new(http.clone());
        let neoforge_server = NeoforgeServerManager::new(http.clone());

        let val = CreeperInner {
            args,
            config,
            artifact,
            vanilla,
            vanilla_server,
            http,
            registry,
            index_cache: IndexCache::new(),
            neoforge,
            neoforge_server,
            game,
            user,
            fabric,
            intermediary,
        };
        Ok(Self(Arc::new(val)))
    }

    pub async fn execute(&self, cmd: impl Execute) -> anyhow::Result<()> {
        cmd.execute(self).await
    }

    pub async fn update_all(&self) -> anyhow::Result<()> {
        if self.args.offline {
            info!("skipping update because offline mode enabled");
            return Ok(());
        }

        self.update_registry().await?;
        self.update_vanilla().await?;
        self.update_vanilla_server().await?;
        self.update_neoforge().await?;
        self.update_fabric().await?;
        self.update_intermediary().await?;
        self.update_neoforge_server().await?;
        Ok(())
    }
}

#[derive(Clone, Debug, Parser)]
pub struct Args {
    /// Path to the config file.
    ///
    /// If not specified, will default to `$CONFIG_DIR/creeper/config.toml`,
    /// where `$CONFIG_DIR` is the user config directory depending on platform, e.g. `$XDG_CONFIG_HOME` on Linux.
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Rewrite the home directory for current minecraft instance.
    ///
    /// If not specified, would recursively look up parent directory from current directory until a `creeper.toml` is found.
    #[arg(short, long)]
    pub dir: Option<PathBuf>,

    /// Run in offline mode.
    ///
    /// If enabled, would prevent network requests and only use cached data.
    /// Note that this may cause some actions to fail.
    /// Also note that the feature is under development,
    /// and there may still be network requests even if this option is enabled.
    #[arg(long, default_value_t = false)]
    pub offline: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            config: None,
            dir: None,
            offline: false,
        }
    }
}

#[serde_inline_default]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    /// URL to the package registry.
    #[serde_inline_default("https://creeper-registry.pages.dev/".parse().unwrap())]
    #[serde(skip_serializing_if = "is_default_registry")]
    pub registry: Url,

    /// Limit number of parallel downloads.
    #[serde_inline_default(4)]
    #[serde(skip_serializing_if = "is_default_parallel_download")]
    pub parallel_download: usize,

    #[serde_inline_default(false)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub use_bmclapi: bool,
}

fn is_default_registry(registry: &Url) -> bool {
    registry == &"https://creeper-registry.pages.dev/".parse().unwrap()
}

fn is_default_parallel_download(parallel_download: &usize) -> bool {
    *parallel_download == 4
}

impl Default for Config {
    fn default() -> Self {
        Self {
            registry: "https://creeper-registry.pages.dev/".parse().unwrap(),
            parallel_download: 4,
            use_bmclapi: false,
        }
    }
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
#[command(version)]
pub struct Command {
    #[clap(flatten)]
    pub args: Args,

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
pub enum SubCommand {
    #[command(subcommand)]
    Tool(Tool),

    Add(cmd::Add),

    Launch(cmd::Launch),

    Install(cmd::Install),

    Nuke(cmd::Nuke),

    Login(cmd::Login),

    Init(cmd::Init),

    #[command(subcommand)]
    Dev(Dev),

    Complete(cmd::Complete),

    #[clap(hide = true)]
    AwwMan,
}

impl Execute for SubCommand {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        match self {
            SubCommand::Tool(tool) => lib.execute(tool).await,
            SubCommand::AwwMan => Ok(println!("{CREEPER_TEXT_ART}")),
            SubCommand::Install(install) => lib.execute(install).await,
            SubCommand::Launch(launch) => lib.execute(launch).await,
            SubCommand::Nuke(nuke) => lib.execute(nuke).await,
            SubCommand::Login(login) => lib.execute(login).await,
            SubCommand::Init(init) => lib.execute(init).await,
            SubCommand::Add(add) => lib.execute(add).await,
            SubCommand::Dev(_dev) => todo!(),
            SubCommand::Complete(complete) => lib.execute(complete).await,
        }
    }
}

fn main() {
    let Command {
        args,
        cmd,
        log_level,
        verbose,
        noisy,
    } = Command::parse();

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

    let creeper = run.block_on(Creeper::new(args)).unwrap_or_else(fatal!());

    run.block_on(creeper.execute(cmd)).unwrap_or_else(fatal!());
}
