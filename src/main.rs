mod artifact;
mod asset;
mod builtin;
mod checksum;
mod cmd;
mod config;
mod dev;
mod fabric;
mod game;
mod id;
mod index;
mod init;
mod inquire;
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
mod singleflight;
mod tool;
mod user;
mod util;
mod vanilla;
mod yggdrasil;
mod zip;

use clap::Parser;
use fabric_meta_api::FabricMetaClient;
use reqwest::Client;
use std::{ops::Deref, sync::Arc};
use stop::fatal;
use tokio::runtime;
use tokio_throttle::Throttle;
use tracing::{info, level_filters::LevelFilter};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::{
    EnvFilter, Layer, fmt, layer::SubscriberExt, reload, util::SubscriberInitExt,
};

use crate::{
    artifact::ArtifactManager,
    fabric::{FabricManager, IntermediaryManager},
    game::GameManager,
    index::IndexCache,
    inquire::{InquireManager, make_filter},
    java::JavaManager,
    mc::{ClientManager, MinecraftManager, ServerManager},
    neoforge::{NeoforgeClientManager, NeoforgeManager, NeoforgeServerManager},
    pbar::StdioWriter,
    registry::Registry,
    user::UserManager,
    vanilla::{VanillaManager, VanillaServerManager},
};

pub use prelude::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct Creeper(Arc<CreeperInner>);

impl Deref for Creeper {
    type Target = CreeperInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct CreeperInner {
    pub args: Args,
    pub config: Config,

    stdio: StdioWriter,
    inquire: InquireManager,

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

impl Creeper {
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

fn main() {
    let Command {
        args,
        cmd,
        log,
        log_level,
    } = Command::parse();

    let log_level = log_level.determine().unwrap_or_else(fatal!());

    let layer = IndicatifLayer::new();

    let (stdout, stderr) = (layer.get_stdout_writer(), layer.get_stderr_writer());

    let (filter, handle) = reload::Layer::new(make_filter(|_| true));

    tracing_subscriber::registry()
        .with(EnvFilter::new(log))
        .with(LevelFilter::from_level(log_level))
        .with(
            fmt::layer()
                .with_writer(layer.get_stderr_writer())
                .with_filter(filter),
        )
        .with(layer)
        .init();

    let run = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(fatal!());

    let creeper = run.block_on(Creeper::new(args)).unwrap_or_else(fatal!());

    creeper.set_stdout(stdout);
    creeper.set_stderr(stderr);
    creeper.blocking_inquire_filter(handle);

    run.block_on(creeper.execute(cmd)).unwrap_or_else(fatal!());
}
