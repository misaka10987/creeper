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
mod java;
mod launch;
mod lock;
mod mc;
mod ms;
mod neoforge;
mod pack;
mod path;
mod pbar;
mod prelude;
mod pubgrub;
mod registry;
mod single;
mod tool;
mod user;
mod util;
mod vanilla;
mod yggdrasil;
mod zip;

use clap::Parser;
use fabric_meta_api::FabricMetaClient;
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
use tokio_throttle::{IntoThrottle, Throttle};
use tracing::{Level, info, level_filters::LevelFilter};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
use url::Url;

use crate::{
    artifact::ArtifactManager,
    cmd::Execute,
    fabric::{FabricManager, IntermediaryManager},
    game::GameManager,
    index::IndexCache,
    java::JavaManager,
    mc::{ClientManager, ManifestClient, MinecraftManager, ServerManager},
    neoforge::{NeoforgeClientManager, NeoforgeManager, NeoforgeServerManager},
    path::{creeper_config_dir, init_creeper_dirs},
    pbar::StdioWriter,
    registry::Registry,
    user::UserManager,
    vanilla::{VanillaManager, VanillaServerManager},
};

pub use prelude::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct CreeperInner {
    pub args: Args,
    pub config: Config,

    stdio: StdioWriter,

    http: Throttle<Client>,
    // manifest: ManifestClient,
    fabric_meta: FabricMetaClient,
    artifact: ArtifactManager,

    game: GameManager,

    user: UserManager,
    java: JavaManager,

    registry: Registry,
    index_cache: IndexCache,

    // builtin packages
    minecraft: MinecraftManager,
    client: ClientManager,
    server: ServerManager,

    vanilla: VanillaManager,
    vanilla_server: VanillaServerManager,

    neoforge: NeoforgeManager,
    neoforge_client: NeoforgeClientManager,
    neoforge_server: NeoforgeServerManager,

    fabric: FabricManager,
    intermediary: IntermediaryManager,
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

        let http = Client::default().into_throttle(config.parallel_download);

        let manifest = ManifestClient::new(http.clone());
        let fabric_meta = FabricMetaClient::new(http.clone());

        let registry = Registry::new(config.registry.clone(), http.clone())?;
        let game = GameManager::new(args.dir.clone());

        let minecraft = MinecraftManager::new(manifest.clone());
        let client = ClientManager::new(manifest.clone());
        let server = ServerManager::new(manifest.clone());

        let vanilla = VanillaManager::new(manifest.clone());
        let vanilla_server = VanillaServerManager::new(manifest.clone());

        let neoforge = NeoforgeManager::new(http.clone());
        let neoforge_client = NeoforgeClientManager::new(http.clone());
        let neoforge_server = NeoforgeServerManager::new(http.clone());

        let fabric = FabricManager::new(fabric_meta.clone(), config.parallel_download);
        let intermediary = IntermediaryManager::new(fabric_meta.clone());

        let artifact = ArtifactManager::new(http.clone(), args.offline).await?;
        let user = UserManager::new();
        let java = JavaManager::new();

        let val = CreeperInner {
            args,
            config,

            stdio: StdioWriter::default(),

            artifact,
            http,
            fabric_meta,
            registry,
            index_cache: IndexCache::new(),
            game,
            java,
            user,

            minecraft,
            client,
            server,

            vanilla,
            vanilla_server,

            neoforge,
            neoforge_client,
            neoforge_server,
            fabric,
            intermediary,
            // manifest,
        };
        Ok(Self(Arc::new(val)))
    }

    pub async fn execute(&self, cmd: impl Execute) -> anyhow::Result<()> {
        cmd.execute(self).await
    }

    pub async fn update(&self) -> anyhow::Result<()> {
        if self.args.offline {
            info!("skipping update because offline mode enabled");
            return Ok(());
        }

        self.update_registry().await?;
        self.update_builtin_index().await?;

        Ok(())
    }
}

#[derive(Clone, Parser)]
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

fn main() {
    let Command {
        args,
        cmd,
        log,
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

    let (stdout, stderr) = (layer.get_stdout_writer(), layer.get_stderr_writer());

    tracing_subscriber::registry()
        .with(EnvFilter::new(log))
        .with(LevelFilter::from_level(log_level))
        .with(fmt::layer().with_writer(layer.get_stderr_writer()))
        .with(layer)
        .init();

    let run = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(fatal!());

    let creeper = run.block_on(Creeper::new(args)).unwrap_or_else(fatal!());

    creeper.set_stdout(stdout);
    creeper.set_stderr(stderr);

    run.block_on(creeper.execute(cmd)).unwrap_or_else(fatal!());
}
