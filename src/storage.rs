use std::path::{Path, PathBuf};

use anyhow::bail;
use reqwest::{IntoUrl, Response};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, SqlitePool, prelude::FromRow, sqlite::SqliteConnectOptions};
use sqlx::{query, query_as};
use tokio::fs::{File, create_dir_all, metadata};
use tokio::io::{AsyncWriteExt, BufWriter};
use tracing::{Span, debug, info, instrument, trace};
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::checksum::{HashFunc, blake3};
use crate::{Checksum, PROGRESS_STYLE_DOWNLOAD, creeper_cache, creeper_local_data, mv};

#[derive(Clone, Serialize, Deserialize, FromRow)]
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
            .join("download")
            .join(&blake3[..2])
            .join(blake3);
        Ok(path)
    }
}

const DB_INIT_QUERY: &str = include_str!("storage.sql");

pub struct Storage {
    index: SqlitePool,
    http: reqwest::Client,
}

impl Storage {
    pub async fn new() -> anyhow::Result<Self> {
        let path = creeper_local_data()?.join("storage-index.db");
        let opt = SqliteConnectOptions::default()
            .filename(path)
            .create_if_missing(true);
        let index = SqlitePool::connect_with(opt).await?;
        index.execute(DB_INIT_QUERY).await?;
        let val = Self {
            index,
            http: Default::default(),
        };
        Ok(val)
    }

    async fn http_get(&self, url: impl IntoUrl) -> anyhow::Result<Response> {
        let req = self.http.get(url).build()?;
        let res = self.http.execute(req).await?;
        Ok(res)
    }

    async fn find(&self, checksum: &Checksum) -> anyhow::Result<Option<Artifact>> {
        let found = query_as("SELECT * FROM artifact WHERE ? = ?")
            .bind(checksum.function.to_string())
            .bind(&checksum.hex_hash)
            .fetch_optional(&self.index)
            .await?;
        Ok(found)
    }

    async fn add(&self, artifact: &Artifact) -> anyhow::Result<()> {
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
    pub async fn store(
        &self,
        file: impl AsRef<Path>,
        name: String,
        src: String,
        checksum: impl IntoIterator<Item = Checksum>,
    ) -> anyhow::Result<Artifact> {
        let blake3 = blake3(&file).await?;
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

    #[instrument(skip(self, artifact), fields(artifact.name = &artifact.name))]
    pub async fn retrieve(&self, artifact: &Artifact) -> anyhow::Result<PathBuf> {
        let blake3 = Checksum::blake3(artifact.blake3.clone());
        if let Some(found) = self.find(&blake3).await? {
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
    pub async fn download(
        &self,
        name: String,
        src: String,
        len: Option<u64>,
        checksum: impl IntoIterator<Item = Checksum>,
    ) -> anyhow::Result<Artifact> {
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

        info!("download finished");

        let art = self.store(&path, name, src, checksum).await?;

        Ok(art)
    }
}
