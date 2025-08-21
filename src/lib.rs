pub mod cmd;
pub mod inst;
pub mod java;
pub mod launch;
pub mod pack;
pub mod prelude;
pub mod user;

use std::{env::current_dir, path::PathBuf, sync::OnceLock};

use clap::Parser;
use stop::stop;

pub use prelude::*;

use crate::cmd::{Execute, SubCommand};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct Creeper {
    pub args: CreeperArgs,
    inst: OnceLock<Inst>,
}

impl Creeper {
    pub fn new(args: CreeperArgs) -> Self {
        let val = Self {
            args,
            inst: OnceLock::new(),
        };
        val
    }

    pub async fn req_inst(&self) -> &Inst {
        if let Some(inst) = self.inst.get() {
            return inst;
        }
        let dir = self
            .args
            .home
            .clone()
            .or(find_inst_dir(current_dir().unwrap_or_else(stop!())))
            .unwrap_or_else(|| stop!("not in any game instance"));
        let inst = Inst::load(&dir).await.unwrap_or_else(stop!());
        self.inst.get_or_init(|| inst)
    }

    pub async fn execute(&self, cmd: &impl Execute) {
        Execute::execute(&self, cmd).await.unwrap_or_else(stop!())
    }

    pub async fn run(&self) {
        self.execute(&self.args.cmd).await
    }
}

/// Minecraft Package Manager.
#[derive(Clone, Debug, Parser)]
#[command(version)]
pub struct CreeperArgs {
    /// Rewrite the home directory for current minecraft instance.
    ///
    /// If unspecified, would recursively look up parent directory from current directory until a `creeper.toml` is found.
    #[arg(long)]
    pub home: Option<PathBuf>,
    #[command(subcommand)]
    pub cmd: SubCommand,
}
