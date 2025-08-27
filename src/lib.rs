pub mod checksum;
pub mod cmd;
pub mod http;
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
    env::current_dir,
    fmt::Write,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, OnceLock},
};

use anyhow::anyhow;
use clap::Parser;
use indicatif::{FormattedDuration, ProgressState};
use reqwest::Client;

pub use prelude::*;
use tokio::fs::{File, copy, create_dir_all, remove_file, rename};
use tracing_indicatif::style::ProgressStyle;

use crate::{storage::StorageManager, vanilla::VanillaManager};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct CreeperInner {
    pub args: CreeperConfig,
    storage: StorageManager,
    vanilla: VanillaManager,
    http: Client,
    inst_dir: OnceLock<PathBuf>,
    inst: OnceLock<Inst>,
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
            storage: StorageManager::new().await?,
            vanilla: VanillaManager::new(),
            http: Default::default(),
            inst_dir: OnceLock::new(),
            inst: OnceLock::new(),
        };
        Ok(Self(Arc::new(val)))
    }

    pub fn working_dir(&self) -> anyhow::Result<PathBuf> {
        Ok(self.args.working_dir.clone().unwrap_or(current_dir()?))
    }

    pub fn inst_dir(&self) -> anyhow::Result<&PathBuf> {
        if let Some(dir) = self.inst_dir.get() {
            return Ok(dir);
        }
        let wd = self.working_dir()?;
        let found = Inst::find_dir(wd).ok_or(anyhow!("not in any game instance"))?;
        Ok(self.inst_dir.get_or_init(|| found))
    }

    pub async fn inst(&self) -> anyhow::Result<&Inst> {
        if let Some(inst) = self.inst.get() {
            return Ok(inst);
        }
        let inst = Inst::load(self.inst_dir()?).await?;
        Ok(self.inst.get_or_init(|| inst))
    }
}

impl AsRef<Client> for Creeper {
    fn as_ref(&self) -> &Client {
        &self.http
    }
}

impl AsRef<StorageManager> for Creeper {
    fn as_ref(&self) -> &StorageManager {
        &self.storage
    }
}

impl AsRef<VanillaManager> for Creeper {
    fn as_ref(&self) -> &VanillaManager {
        &self.vanilla
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
