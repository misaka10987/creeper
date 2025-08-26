pub mod checksum;
pub mod cmd;
pub mod inst;
pub mod java;
pub mod launch;
pub mod lock;
pub mod mc;
pub mod pack;
pub mod prelude;
pub mod storage;
pub mod user;
pub mod vanilla;

use std::{
    collections::HashMap,
    env::current_dir,
    fmt::Write,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, OnceLock},
};

use anyhow::anyhow;
use clap::Parser;
use indicatif::{FormattedDuration, ProgressState};
use mc_launchermeta::{version::Version as McVersion, version_manifest::Manifest};
use reqwest::{Client, IntoUrl, Response};

use semver::Version;

pub use prelude::*;
use tokio::{
    fs::{File, copy, create_dir_all, remove_file, rename},
    sync::RwLock,
};
use tracing_indicatif::style::ProgressStyle;

use crate::storage::Storage;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct CreeperInner {
    pub args: CreeperConfig,
    pub storage: Storage,
    http: Client,
    inst: OnceLock<Inst>,
    manifest: OnceLock<Manifest>,
    mc_version: RwLock<HashMap<Version, McVersion>>,
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
        let val = CreeperInner {
            args,
            storage: Storage::new().await?,
            http: Default::default(),
            inst: OnceLock::new(),
            manifest: OnceLock::new(),
            mc_version: RwLock::new(HashMap::new()),
        };
        Ok(Self(Arc::new(val)))
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

fn pb_eta(state: &ProgressState, w: &mut dyn Write) {
    write!(w, "{}", FormattedDuration(state.eta())).unwrap()
}

static PROGRESS_STYLE_DOWNLOAD: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::with_template("{spinner:.green} {msg} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes:>11}/{total_bytes:<11} ETA {eta:<8}")
        .unwrap()
        .with_key("eta", pb_eta)
        .progress_chars("=> ")
});

async fn mv(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> anyhow::Result<()> {
    if let Some(parent) = dst.as_ref().parent() {
        create_dir_all(parent).await?;
    }
    File::create(&dst).await?;

    let rename = rename(&src, &dst).await;
    match rename {
        Ok(_) => return Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {}
        e => e?,
    }
    copy(&src, &dst).await?;
    remove_file(&src).await?;
    Ok(())
}

fn creeper_local_data() -> anyhow::Result<PathBuf> {
    let dir = dirs::data_local_dir()
        .ok_or(anyhow!("missing local data directory"))?
        .join("creeper");
    Ok(dir)
}

fn creeper_cache() -> anyhow::Result<PathBuf> {
    let dir = dirs::cache_dir()
        .ok_or(anyhow!("missing cache directory"))?
        .join("creeper");
    Ok(dir)
}
