pub mod cmd;
pub mod inst;
pub mod java;
pub mod launch;
pub mod pack;
pub mod prelude;
pub mod user;

use std::{collections::HashMap, env::current_dir, path::PathBuf, sync::OnceLock};

use anyhow::anyhow;
use clap::Parser;
use mc_launchermeta::{
    VERSION_MANIFEST_URL, version::Version as McVersion, version_manifest::Manifest,
};
use reqwest::{Client, IntoUrl, Response};
use semver::Version;

pub use prelude::*;
use tokio::sync::RwLock;
use tracing::info;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct Creeper {
    pub args: CreeperConfig,
    http: Client,
    inst: OnceLock<Inst>,
    manifest: OnceLock<Manifest>,
    mc_version: RwLock<HashMap<Version, McVersion>>,
}

impl Creeper {
    pub fn new(args: CreeperConfig) -> Self {
        let val = Self {
            args,
            http: Default::default(),
            inst: OnceLock::new(),
            manifest: OnceLock::new(),
            mc_version: RwLock::new(HashMap::new()),
        };
        val
    }

    async fn http_get(&self, url: impl IntoUrl) -> anyhow::Result<Response> {
        let req = self.http.get(url).build()?;
        let res = self.http.execute(req).await?;
        Ok(res)
    }

    async fn load_inst(&self) -> anyhow::Result<&Inst> {
        let dir = current_dir()?;
        let dir = self
            .args
            .working_dir
            .to_owned()
            .or(find_inst_dir(dir))
            .ok_or(anyhow!("not in any game instance"))?;
        let inst = Inst::load(dir).await?;
        Ok(self.inst.get_or_init(|| inst))
    }

    pub async fn inst(&self) -> anyhow::Result<&Inst> {
        if let Some(inst) = self.inst.get() {
            return Ok(inst);
        }
        self.load_inst().await
    }

    async fn fetch_manifest(&self) -> anyhow::Result<&Manifest> {
        info!("synchronizing minecraft version manifest");
        let manifest = self.http_get(VERSION_MANIFEST_URL).await?.json().await?;
        Ok(self.manifest.get_or_init(|| manifest))
    }

    pub async fn manifest(&self) -> anyhow::Result<&Manifest> {
        if let Some(manifest) = self.manifest.get() {
            return Ok(manifest);
        }
        self.fetch_manifest().await
    }

    async fn fetch_mc_version(&self, version: Version) -> anyhow::Result<McVersion> {
        let manifest = self.manifest().await?;
        let url = manifest
            .get_version(&version.to_string())
            .ok_or(anyhow!("minecraft version {version} not found in manifest"))?
            .url
            .to_owned();
        let mc_version = self.http_get(url).await?.json::<McVersion>().await?;
        self.mc_version
            .write()
            .await
            .insert(version, mc_version.clone());
        Ok(mc_version)
    }

    pub async fn mc_version(&self, version: Version) -> anyhow::Result<McVersion> {
        if let Some(mc_version) = self.mc_version.read().await.get(&version) {
            return Ok(mc_version.clone());
        }
        self.fetch_mc_version(version).await
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
