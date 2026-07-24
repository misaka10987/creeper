mod db;
mod download;
mod parallel;

use std::fmt::Display;
use std::iter::once;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail};
use base64::{Engine, prelude::BASE64_URL_SAFE};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, SqlitePool, prelude::FromRow, sqlite::SqliteConnectOptions};
use tokio::fs::{File, copy, create_dir_all, metadata, try_exists};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::Semaphore;
use tracing::{Span, debug, info, instrument, trace};
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::path::{creeper_cache_dir, creeper_data_dir};
use crate::pbar::PROGRESS_STYLE_DOWNLOAD;
use crate::util::{mv, set_readonly};
use crate::{
    Checksum, Creeper,
    checksum::{HashFunc, blake3},
};
use crate::{checksum, symlink_auto};

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, FromRow)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub blake3: String,

    pub name: String,

    pub src: Option<String>,

    pub len: u64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub md5: Option<String>,
}

impl Display for Artifact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (#{})", self.name, &self.blake3[..8])
    }
}

impl Artifact {
    pub fn new(blake3: String, name: String, src: Option<String>, len: u64) -> Self {
        Self {
            blake3,
            name,
            src,
            len,
            sha1: None,
            sha256: None,
            md5: None,
        }
    }

    pub fn try_extend(&mut self, other: impl Iterator<Item = Self>) -> anyhow::Result<()> {
        for art in other {
            let Self {
                blake3,
                name: _,
                src: _,
                len,
                sha1,
                sha256,
                md5,
            } = art;
            if self.blake3 != blake3
                || self.len != len
                || matches!((&self.sha1, &sha1), (Some(x), Some(y)) if x != y)
                || matches!((&self.sha256, &sha256), (Some(x), Some(y)) if x != y)
                || matches!((&self.md5, &md5), (Some(x), Some(y)) if x != y)
            {
                bail!("different artifacts can not be extended");
            }
            self.sha1 = sha1;
            self.sha256 = sha256;
            self.md5 = md5;
        }
        Ok(())
    }

    pub fn checksum(self) -> impl Iterator<Item = Checksum> {
        Some(Checksum::blake3(self.blake3))
            .into_iter()
            .chain(self.sha1.map(Checksum::sha1))
            .chain(self.sha256.map(Checksum::sha256))
    }

    pub fn path(&self) -> anyhow::Result<PathBuf> {
        Self::storage_path(&self.blake3)
    }

    pub fn has_checksum(&self, checksum: HashFunc) -> bool {
        match checksum {
            HashFunc::Blake3 => true,
            HashFunc::Sha1 => self.sha1.is_some(),
            HashFunc::Sha256 => self.sha256.is_some(),
        }
    }

    pub fn affix_checksum(&mut self, checksum: Checksum) {
        match checksum.function {
            crate::checksum::HashFunc::Blake3 => {
                debug!("`affix_checksum` called with blake3 checksum, this does nothing")
            }
            crate::checksum::HashFunc::Sha1 => self.sha1 = Some(checksum.hex_hash),
            crate::checksum::HashFunc::Sha256 => self.sha256 = Some(checksum.hex_hash),
        }
    }

    pub fn storage_path(blake3: &str) -> anyhow::Result<PathBuf> {
        let path = creeper_data_dir()?
            .join("artifact")
            .join(&blake3[..2])
            .join(blake3);
        Ok(path)
    }

    pub async fn verify(&self, file: impl AsRef<Path>) -> anyhow::Result<bool> {
        let b3 = blake3(file).await?;
        Ok(b3 == self.blake3)
    }
}

const DB_INIT_QUERY: &str = include_str!("init.sql");

pub struct ArtifactManager {
    pub offline: bool,

    http: Client,

    index: SqlitePool,

    semaphore: Semaphore,
}

impl ArtifactManager {
    pub async fn new(
        http: Client,
        offline: bool,
        parallel_download: usize,
    ) -> anyhow::Result<Self> {
        let path = creeper_data_dir()?.join("artifact.db");
        let opt = SqliteConnectOptions::default()
            .filename(path)
            .create_if_missing(true);
        let index = SqlitePool::connect_with(opt).await?;
        index.execute(DB_INIT_QUERY).await?;

        let semaphore = Semaphore::new(parallel_download);

        let val = Self {
            index,
            http,
            offline,
            semaphore,
        };
        Ok(val)
    }

    async fn get(&self, blake3: &str) -> anyhow::Result<Option<Artifact>> {
        self.select(HashFunc::Blake3, blake3).await
    }

    async fn get_checksum(&self, checksum: &Checksum) -> anyhow::Result<Option<Artifact>> {
        self.select(checksum.function, &checksum.hex_hash).await
    }

    async fn has_storage(&self, blake3: &str) -> anyhow::Result<bool> {
        let path = Artifact::storage_path(blake3)?;
        if try_exists(&path).await? {
            if checksum::blake3(&path).await? == blake3 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn add_or_update(&self, art: Artifact) -> anyhow::Result<()> {
        if let Some(a) = self.get(&art.blake3).await? {
            let mut new = a.clone();
            new.try_extend(once(art))?;

            if a == new {
                trace!("nothing to update for artifact {}", a.blake3);
                return Ok(());
            }

            self.update(&new).await
        } else {
            self.insert(&art).await
        }
    }

    /// See [`Creeper::retrieve_artifact`].
    #[instrument(skip(self, art), fields(artifact = &art.name))]
    async fn retrieve(&self, art: &Artifact) -> anyhow::Result<PathBuf> {
        let path = art.path()?;

        if self.has_storage(&art.blake3).await? {
            self.add_or_update(art.clone()).await?;
            return Ok(path);
        }

        if self.offline {
            bail!("offline mode enabled, cannot retrieve missing artifact {art}")
        }

        let src = match &art.src {
            Some(x) => x,
            None => bail!("missing download source"),
        };

        debug!("downloading from {}", src);

        let cache = creeper_cache_dir()?.join(BASE64_URL_SAFE.encode(&src));
        trace!("download caching to {cache:?}");
        create_dir_all(cache.parent().unwrap()).await?;

        let semaphore = self.semaphore.acquire().await?;

        let mut writer = BufWriter::new(File::create(&cache).await?);

        let span = Span::current();
        let trunc: String = art.name.chars().take(8).collect();
        span.pb_set_message(&trunc);
        span.pb_set_style(&PROGRESS_STYLE_DOWNLOAD);
        span.pb_set_length(art.len);

        let req = self.http.get(src).build()?;
        let mut res = self.http.execute(req).await?;

        while let Some(chunk) = res.chunk().await? {
            writer.write_all(&chunk).await?;
            span.pb_inc(chunk.len() as u64);
        }

        writer.shutdown().await?;

        drop(semaphore);

        info!("download finished");

        set_readonly(&cache).await?;

        if !art.verify(&cache).await? {
            bail!("invalid download");
        }

        self.add_or_update(art.clone()).await?;

        mv(&cache, &path).await?;

        Ok(path)
    }
}

impl Creeper {
    /// Retrieve an artifact and return its storage path.
    ///
    /// This automatically downloads the file and updates the artifact database, if necessary.
    ///
    /// # Important Note
    ///
    /// Despite the fact that multiple checksums can be defined in an `Artifact`,
    /// the method only guarantees the output file matches the blake3 checksum.
    /// Any other checksums will be regarded as automatically correct,
    /// if the blake3 checksum matches.
    ///
    /// This implies that, if the method is called with an `Artifact` with a false optional checksum,
    /// the method will still succeed and potentially write poisoned records into the artifact database.
    pub async fn retrieve_artifact(&self, art: &Artifact) -> anyhow::Result<PathBuf> {
        self.artifact.retrieve(art).await
    }

    /// Retrieve an artifact and create a soft link to it at the specified path.
    /// Creating parent directories if necessary.
    ///
    /// If `path` exists and is a soft link matching the specified artifact, this function does only update the artifact database.
    /// Repeated calls to this function is guaranteed idempotent.
    ///
    /// If `path` exists and is a soft link that does not match the specified artifact,
    /// **or** if `path` exists and is not a soft link, the function fails.
    ///
    /// See [`Self::retrieve_artifact`] for details and caveats.
    pub async fn retrieve_artifact_to(
        &self,
        art: &Artifact,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let dst = path.as_ref();
        trace!(
            "retrieving artifact {} to {}",
            &art.blake3[..6],
            dst.display()
        );

        if dst.exists() {
            if !dst.is_symlink() {
                bail!(
                    "can not retrieve artifact to {} because it already exists and is not a soft link",
                    dst.display()
                );
            }

            if !art.verify(dst).await? {
                bail!(
                    "can not retrieve artifact to {}, refusing to overwrite",
                    dst.display()
                );
            }

            trace!(
                "found valid artifact at {}, skipping retrieval",
                dst.display()
            );

            self.artifact.add_or_update(art.clone()).await?;

            return Ok(());
        }

        let src = self.retrieve_artifact(art).await?;

        if let Some(parent) = dst.parent() {
            create_dir_all(parent).await?;
        }

        symlink_auto(src, dst).await?;

        Ok(())
    }

    /// Download a file from specified URL and store it in the artifact storage.
    ///
    /// If `len` and `checksum` are specified, it is guaranteed that the downloaded file matches the constraints.
    pub async fn download(
        &self,
        name: String,
        src: String,
        len: Option<u64>,
        checksum: impl IntoIterator<Item = Checksum> + Send,
    ) -> anyhow::Result<Artifact> {
        self.artifact.download(name, src, len, checksum).await
    }

    /// Store a file to the artifact storage.
    ///
    /// # Note
    ///
    /// The file will not have a download source.
    pub async fn store_artifact(&self, file: impl AsRef<Path>) -> anyhow::Result<Artifact> {
        let file = file.as_ref();

        let b3 = blake3(file).await?;

        if let Some(art) = self.artifact.get(&b3).await? {
            return Ok(art);
        }

        let name = file
            .file_name()
            .ok_or(anyhow!("missing filename"))?
            .to_str()
            .ok_or(anyhow!("invalid filename"))?;

        let metadata = metadata(file).await?;
        let len = metadata.len();

        let art = Artifact::new(b3, name.into(), None, len);

        if !self.artifact.has_storage(&art.blake3).await? {
            let storage = art.path()?;
            create_dir_all(storage.parent().unwrap()).await?;
            copy(file, &storage).await?;
            set_readonly(&storage).await?;
        }

        self.artifact.insert(&art).await?;

        Ok(art)
    }
}
