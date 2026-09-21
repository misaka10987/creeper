use clap::Parser;
use fabric_meta_api::FabricMetaClient;
use reqwest::Client;
use std::{path::PathBuf, sync::Arc};
use tokio_throttle::IntoThrottle;

use crate::{
    Creeper, CreeperInner,
    artifact::ArtifactManager,
    fabric::{FabricManager, IntermediaryManager},
    game::GameManager,
    index::IndexCache,
    inquire::InquireManager,
    java::JavaManager,
    mc::{ClientManager, ManifestClient, MinecraftManager, ServerManager},
    neoforge::{NeoforgeClientManager, NeoforgeManager, NeoforgeServerManager},
    path::{creeper_config_dir, init_creeper_dirs},
    pbar::StdioWriter,
    registry::Registry,
    user::UserManager,
    vanilla::{VanillaManager, VanillaServerManager},
};

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

impl Creeper {
    pub async fn new(args: Args) -> anyhow::Result<Self> {
        init_creeper_dirs().await?;

        let path = args
            .config
            .clone()
            .unwrap_or(creeper_config_dir()?.join("config.toml"));

        let config = Self::load_config(path).await?;

        let http = Client::default().into_throttle(config.parallel_http);

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

        let fabric = FabricManager::new(fabric_meta.clone(), config.parallel_job);
        let intermediary = IntermediaryManager::new(fabric_meta.clone());

        let artifact = ArtifactManager::new(http.clone(), args.offline).await?;
        let user = UserManager::new();
        let java = JavaManager::new();

        let val = CreeperInner {
            args,
            config,

            stdio: StdioWriter::default(),
            inquire: InquireManager::new(),

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
}
