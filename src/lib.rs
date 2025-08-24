pub mod checksum;
pub mod cmd;
pub mod inst;
pub mod java;
pub mod launch;
pub mod mc;
pub mod pack;
pub mod prelude;
pub mod storage;
pub mod user;

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
use mc_launchermeta::{
    VERSION_MANIFEST_URL, version::Version as McVersion, version_manifest::Manifest,
};
use reqwest::{Client, IntoUrl, Response};

use semver::Version;

pub use prelude::*;
use tokio::{
    fs::{File, copy, create_dir_all, remove_file, rename},
    sync::RwLock,
    task::JoinSet,
};
use tracing::{Instrument, info};
use tracing_indicatif::style::ProgressStyle;

use crate::{
    mc::{check_class, check_os},
    storage::Storage,
};

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
        info!("synchronizing minecraft {version} version metadata");
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

    pub async fn download_mc_lib(&self, version: Version) -> anyhow::Result<()> {
        let mc_version = self.mc_version(version).await?;

        let arts = mc_version
            // libraries
            .libraries
            .into_iter()
            // apply the rules
            .filter(|x| {
                x.rules.as_ref().is_none_or(|rules| {
                    rules.iter().all(|rule| {
                        if !rule.features.is_empty() {
                            todo!("does not support rules with features")
                        }
                        let os = rule.os.as_ref().is_none_or(check_os);
                        match rule.action {
                            mc_launchermeta::version::rule::RuleAction::Allow => os,
                            mc_launchermeta::version::rule::RuleAction::Disallow => !os,
                        }
                    })
                })
            })
            // entries with artifacts to download
            .filter_map(|x| x.downloads)
            // flatten list of artifacts
            .flat_map(|x| {
                x.classifiers
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|(class, art)| if check_class(&class) { Some(art) } else { None })
                    .chain(x.artifact.into_iter())
            })
            // remove duplication
            .map(|x| (x.sha1.clone(), x))
            .collect::<HashMap<_, _>>();

        info!("downloading {} library artifacts", arts.len());

        let mut set = JoinSet::new();

        for art in arts.into_values() {
            let creeper = self.clone();
            let fut = async move {
                creeper
                    .storage
                    .download(
                        art.path,
                        art.url,
                        Some(art.size),
                        Some(art.sha1).map(Checksum::sha1),
                    )
                    .await
            };
            set.spawn(fut.in_current_span());
        }

        while let Some(res) = set.join_next().await {
            res??;
        }

        Ok(())
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
