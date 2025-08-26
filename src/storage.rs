use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, SqlitePool, prelude::FromRow, sqlite::SqliteConnectOptions};
use sqlx::{query, query_as};
use tokio::fs::{File, create_dir_all, metadata};
use tokio::io::{AsyncWriteExt, BufWriter};
use tracing::{Span, debug, info, instrument, trace, warn};
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::{
    Checksum, PROGRESS_STYLE_DOWNLOAD,
    checksum::{HashFunc, blake3},
    creeper_cache, creeper_local_data,
    http::HttpRequest,
    mv,
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, FromRow)]
pub struct Artifact {
    pub blake3: String,
    pub name: String,
    pub src: String,
    pub len: u64,
    // other checksums
    pub sha1: Option<String>,
    pub sha256: Option<String>,
    pub md5: Option<String>,
}

impl Artifact {
    pub fn new(blake3: String, name: String, src: String, len: u64) -> Self {
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

    pub fn checksum(self) -> impl Iterator<Item = Checksum> {
        Some(self.blake3)
            .map(Checksum::blake3)
            .into_iter()
            .chain(self.sha1.map(Checksum::sha1).into_iter())
            .chain(self.sha256.map(Checksum::sha256).into_iter())
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
        let path = creeper_local_data()?
            .join("storage")
            .join(&blake3[..2])
            .join(blake3);
        Ok(path)
    }
}

const DB_INIT_QUERY: &str = include_str!("storage.sql");

pub struct StorageManager {
    index: SqlitePool,
}

impl StorageManager {
    pub async fn new() -> anyhow::Result<Self> {
        let path = creeper_local_data()?.join("storage-index.db");
        let opt = SqliteConnectOptions::default()
            .filename(path)
            .create_if_missing(true);
        let index = SqlitePool::connect_with(opt).await?;
        index.execute(DB_INIT_QUERY).await?;
        let val = Self { index };
        Ok(val)
    }

    async fn find_checksum(&self, checksum: &Checksum) -> anyhow::Result<Option<Artifact>> {
        // column names can not be parameters
        let query = format!("SELECT * FROM artifact WHERE {} = ?", checksum.function);
        let found = query_as(&query)
            .bind(&checksum.hex_hash)
            .fetch_optional(&self.index)
            .await?;
        Ok(found)
    }

    async fn find(&self, blake3: &str) -> anyhow::Result<Option<Artifact>> {
        let found = query_as("SELECT * FROM artifact WHERE blake3 = ?")
            .bind(blake3)
            .fetch_optional(&self.index)
            .await?;
        Ok(found)
    }

    async fn add(&self, artifact: &Artifact) -> anyhow::Result<()> {
        if self.find(&artifact.blake3).await?.is_some() {
            warn!("duplicate add of artifact, this is likely due to an inefficient design");
            return Ok(());
        }
        query("INSERT INTO artifact (blake3, name, src, len, sha1, sha256, md5) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .bind(&artifact.blake3)
        .bind(&artifact.name)
        .bind(&artifact.src)
        .bind(artifact.len as i64)
        .bind(&artifact.sha1)
        .bind(&artifact.sha256)
        .bind(&artifact.md5)
        .execute(&self.index)
        .await?;
        Ok(())
    }

    #[instrument(skip(self, name, src, checksum), fields(file = file.as_ref().display().to_string()))]
    async fn store(
        &self,
        file: impl AsRef<Path>,
        name: String,
        src: String,
        checksum: impl IntoIterator<Item = Checksum>,
    ) -> anyhow::Result<Artifact> {
        // let storage: &StorageManager = self.as_ref();

        let blake3 = blake3(&file).await?;

        if self.find(&blake3).await?.is_some() {
            return self.affix_checksum(&blake3, checksum).await;
        }

        let len = metadata(&file).await?.len();
        let mut art = Artifact::new(blake3.clone(), name, src, len);
        for checksum in checksum {
            // no need to calculate again for blake3
            if checksum.function == HashFunc::Blake3 && checksum.hex_hash != blake3 {
                bail!("broken file {:?}, expected {checksum}", file.as_ref())
            }
            if !checksum.check(&file).await? {
                bail!("broken file {:?}, expected {checksum}", file.as_ref())
            }
            art.affix_checksum(checksum);
        }

        mv(&file, art.path()?).await?;
        self.add(&art).await?;

        Ok(art)
    }

    async fn affix_checksum(
        &self,
        blake3: &str,
        checksum: impl IntoIterator<Item = Checksum>,
    ) -> anyhow::Result<Artifact> {
        // let storage: &StorageManager = self.as_ref();

        let mut art = self
            .find(blake3)
            .await?
            .ok_or(anyhow!("affix checksum to nonexistent artifact"))?;

        let file = art.path()?;

        let mut added = false;

        for checksum in checksum {
            if art.has_checksum(checksum.function) {
                continue;
            }
            if !checksum.check(&file).await? {
                bail!("incorrect new checksum {checksum} for {file:?}");
            }
            art.affix_checksum(checksum);
            added = true;
        }

        if !added {
            debug!("no changes to checksum");
            return Ok(art);
        }

        query("UPDATE artifact SET sha1 = ?, sha256 = ?, md5 = ? WHERE blake3 = ?")
            .bind(&art.sha1)
            .bind(&art.sha256)
            .bind(&art.md5)
            .bind(blake3)
            .execute(&self.index)
            .await?;
        Ok(art)
    }
}

#[allow(async_fn_in_trait)]
pub trait StorageManage {
    async fn retrieve(&self, artifact: &Artifact) -> anyhow::Result<PathBuf>;
    async fn download(
        &self,
        name: String,
        src: String,
        len: Option<u64>,
        checksum: impl IntoIterator<Item = Checksum>,
    ) -> anyhow::Result<Artifact>;
}

impl<T> StorageManage for T
where
    T: AsRef<StorageManager> + HttpRequest,
{
    #[instrument(skip(self, artifact), fields(artifact.name = &artifact.name))]
    async fn retrieve(&self, artifact: &Artifact) -> anyhow::Result<PathBuf> {
        let storage: &StorageManager = self.as_ref();

        let blake3 = Checksum::blake3(artifact.blake3.clone());
        if let Some(found) = storage.find_checksum(&blake3).await? {
            let path = found.path()?;
            trace!("found at {path:?}, checking file integrity");
            if blake3.check(&path).await? {
                trace!("hashes match");
                return Ok(path);
            }
            trace!("hashes mismatch, removing false file");
        }
        debug!("downloading from {}", artifact.src);

        let art = self
            .download(
                artifact.name.clone(),
                artifact.src.clone(),
                Some(artifact.len),
                artifact.clone().checksum(),
            )
            .await?;

        Ok(art.path()?)
    }

    #[instrument(skip(self, name, len, checksum))]
    async fn download(
        &self,
        name: String,
        src: String,
        len: Option<u64>,
        checksum: impl IntoIterator<Item = Checksum>,
    ) -> anyhow::Result<Artifact> {
        let storage: &StorageManager = self.as_ref();

        let checksums = checksum.into_iter().collect::<Vec<_>>();

        for checksum in &checksums {
            if let Some(art) = storage.find_checksum(checksum).await? {
                debug!("fingerprint found in local storage");
                let art = storage.affix_checksum(&art.blake3, checksums).await?;
                info!("verified file integrity, skipping download");
                return Ok(art);
            }
        }

        let path = creeper_cache()?.join(blake3::hash(src.as_bytes()).to_hex().to_string());

        trace!("download caching to {path:?}");

        create_dir_all(path.parent().unwrap()).await?;
        let mut writer = BufWriter::new(File::create(&path).await?);

        let span = Span::current();
        let trunc: String = name.chars().take(8).collect();
        span.pb_set_message(&trunc);
        span.pb_set_style(&PROGRESS_STYLE_DOWNLOAD);
        span.pb_set_length(len.unwrap_or(0));

        let mut res = self.http_get(&src).await?;

        if len.is_none() {
            span.pb_set_length(res.content_length().unwrap_or(0));
        }

        while let Some(chunk) = res.chunk().await? {
            writer.write_all(&chunk).await?;
            span.pb_inc(chunk.len() as u64);
        }

        writer.shutdown().await?;

        info!("download finished");

        let art = storage.store(&path, name, src, checksums).await?;

        Ok(art)
    }
}
