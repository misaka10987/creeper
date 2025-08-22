pub mod cmd;
pub mod inst;
pub mod java;
pub mod launch;
pub mod pack;
pub mod prelude;
pub mod user;

use std::{env::current_dir, path::PathBuf, sync::OnceLock};

use anyhow::anyhow;
use clap::Parser;
use mc_launchermeta::{VERSION_MANIFEST_URL, version_manifest::Manifest};
use stop::stop;

pub use prelude::*;
use tracing::info;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct Creeper {
    pub args: CreeperConfig,
    inst: OnceLock<Inst>,
    manifest: OnceLock<Manifest>,
}

impl Creeper {
    pub fn new(args: CreeperConfig) -> Self {
        let val = Self {
            args,
            inst: OnceLock::new(),
            manifest: OnceLock::new(),
        };
        val
    }

    pub async fn load_inst(&self) -> anyhow::Result<&Inst> {
        let dir = current_dir()?;
        let dir = self
            .args
            .working_dir
            .to_owned()
            .or(find_inst_dir(dir))
            .ok_or(anyhow!("not in any game instance"))?;
        let inst = Inst::load(dir).await?;
        let inst = self.inst.get_or_init(|| inst);
        Ok(inst)
    }

    pub async fn inst(&self) -> anyhow::Result<&Inst> {
        if let Some(inst) = self.inst.get() {
            return Ok(inst);
        }
        self.load_inst().await
    }

    pub async fn req_inst(&self) -> &Inst {
        self.inst().await.unwrap_or_else(stop!())
    }

    pub async fn fetch_manifest(&self) -> anyhow::Result<&Manifest> {
        info!("synchronizing minecraft version manifest");
        let json = reqwest::get(VERSION_MANIFEST_URL).await?.text().await?;
        let val = serde_json::from_str(&json)?;
        let manifest = self.manifest.get_or_init(|| val);
        Ok(manifest)
    }

    pub async fn manifest(&self) -> anyhow::Result<&Manifest> {
        if let Some(manifest) = self.manifest.get() {
            return Ok(manifest);
        }
        self.fetch_manifest().await
    }

    pub async fn req_manifest(&self) -> &Manifest {
        self.manifest().await.unwrap_or_else(stop!())
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

pub const CREEPER_TEXT_ART : &str = r#"
🟩🟩🟩⬜⬜🟩🟩🟩
🟩🟩🟩🟩🟩🟩🟩⬜
🟩⬛⬛🟩🟩⬛⬛⬜
🟩⬛⬛🟩🟩⬛⬛🟩
🟩🟩🟩⬛⬛⬜🟩🟩
🟩🟩⬛⬛⬛⬛🟩⬜
⬜🟩⬛⬛⬛⬛🟩🟩
🟩🟩⬛🟩🟩⬛🟩🟩
"#;
