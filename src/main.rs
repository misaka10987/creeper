mod artifact;
mod asset;
mod builtin;
mod checksum;
mod cmd;
mod config;
mod dev;
mod exe;
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

use colored::Colorize;
use fabric_meta_api::FabricMetaClient;
use reqwest::Client;
use std::{ops::Deref, process::exit, sync::Arc};
use tokio_throttle::Throttle;
use tracing::info;

use crate::{
    artifact::ArtifactManager,
    exe::execute,
    fabric::{FabricManager, IntermediaryManager},
    game::GameManager,
    index::IndexCache,
    inquire::InquireManager,
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
    match execute() {
        Ok(_) => exit(0),
        Err(e) => {
            eprintln!("{} {e}", "fatal:".bold().red());
            exit(-1)
        }
    }
}
