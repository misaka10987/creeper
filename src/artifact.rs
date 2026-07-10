use std::collections::HashMap;
use std::fmt::Display;
use std::hash::Hash;
use std::iter::once;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, ensure};
use base64::{Engine, prelude::BASE64_URL_SAFE};
use futures::{StreamExt, TryStreamExt, stream};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, query, query_as};
use sqlx::{Executor, SqlitePool, prelude::FromRow, sqlite::SqliteConnectOptions};
use tokio::fs::{File, copy, create_dir_all, metadata, remove_file, try_exists};
use tokio::io::{AsyncWriteExt, BufWriter};
use tracing::{Span, debug, info, instrument, trace, warn};
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

const DB_INIT_QUERY: &str = include_str!("artifact.sql");

pub struct ArtifactManager {
    pub offline: bool,

    http: Client,
    index: SqlitePool,
}

impl ArtifactManager {
    pub async fn new(http: Client, offline: bool) -> anyhow::Result<Self> {
        let path = creeper_data_dir()?.join("artifact.db");
        let opt = SqliteConnectOptions::default()
            .filename(path)
            .create_if_missing(true);
        let index = SqlitePool::connect_with(opt).await?;
        index.execute(DB_INIT_QUERY).await?;
        let val = Self {
            index,
            http,
            offline,
        };
        Ok(val)
    }

    async fn find_checksum(&self, checksum: &Checksum) -> anyhow::Result<Option<Artifact>> {
        // column names can not be parameters
        let query = format!("SELECT * FROM artifact WHERE {} = ?", checksum.function);
        let found = query_as(AssertSqlSafe(query))
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

    async fn has_storage(&self, blake3: &str) -> anyhow::Result<bool> {
        let path = Artifact::storage_path(blake3)?;
        if try_exists(&path).await? {
            if checksum::blake3(&path).await? == blake3 {
                return Ok(true);
            }
        }
        Ok(false)
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

    async fn update(&self, art: &Artifact) -> anyhow::Result<()> {
        let r = query("UPDATE artifact SET sha1 = ?, sha256 = ?, md5 = ? WHERE blake3 = ?")
            .bind(&art.sha1)
            .bind(&art.sha256)
            .bind(&art.md5)
            .bind(&art.blake3)
            .execute(&self.index)
            .await?;

        match r.rows_affected() {
            0 => bail!("no artifact to update"),
            1 => Ok(()),
            _ => panic!("duplicate blake3 (primary key)"),
        }
    }

    async fn add_or_update(&self, art: Artifact) -> anyhow::Result<()> {
        if let Some(a) = self.find(&art.blake3).await? {
            let mut new = a.clone();
            new.try_extend(once(art))?;

            if a == new {
                trace!("nothing to update for artifact {}", a.blake3);
                return Ok(());
            }

            self.update(&new).await
        } else {
            self.add(&art).await
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

        info!("download finished");

        set_readonly(&cache).await?;

        if !art.verify(&cache).await? {
            bail!("invalid download");
        }

        self.add_or_update(art.clone()).await?;

        mv(&cache, &path).await?;

        Ok(path)
    }

    /// See [`Creeper::download`].
    #[instrument(skip(self, name, len, checksum))]
    async fn download(
        &self,
        name: String,
        src: String,
        len: Option<u64>,
        checksum: impl IntoIterator<Item = Checksum> + Send,
    ) -> anyhow::Result<Artifact> {
        let checksums = checksum.into_iter().collect::<Vec<_>>();

        // if any of the specified checksums already exists in the database,
        // skip downloading and verify the file with remaining checksums
        for checksum in &checksums {
            if let Some(mut art) = self.find_checksum(checksum).await? {
                debug!("fingerprint found in local storage");

                let path = self.retrieve(&art).await?;

                let func = checksum.function;

                for checksum in checksums {
                    // because the `retrieve` method already checks blake3,
                    // no need to calculate again
                    if checksum.function == HashFunc::Blake3 {
                        ensure!(
                            checksum.hex_hash == art.blake3,
                            "blake3 mismatch while {func} match"
                        );
                        continue;
                    }

                    if !checksum.check(&path).await? {
                        bail!("incorrect checksum for {path:?}, expected {checksum}");
                    }

                    art.affix_checksum(checksum);
                }

                self.add_or_update(art.clone()).await?;

                return Ok(art);
            }
        }

        if self.offline {
            bail!("offline mode enabled, cannot download {src}");
        }

        let cache = creeper_cache_dir()?
            .join("download")
            .join(BASE64_URL_SAFE.encode(&src));
        trace!("download caching to {cache:?}");
        create_dir_all(cache.parent().unwrap()).await?;

        if try_exists(&cache).await? {
            // TODO: continue download if the file is incomplete
            remove_file(&cache).await?;
        }

        let mut writer = BufWriter::new(File::create(&cache).await?);

        let span = Span::current();
        let trunc: String = name.chars().take(8).collect();
        span.pb_set_message(&trunc);
        span.pb_set_style(&PROGRESS_STYLE_DOWNLOAD);
        span.pb_set_length(len.unwrap_or(0));

        let req = self.http.get(&src).build()?;
        let mut res = self.http.execute(req).await?;

        if len.is_none() {
            span.pb_set_length(res.content_length().unwrap_or(0));
        }

        while let Some(chunk) = res.chunk().await? {
            writer.write_all(&chunk).await?;
            span.pb_inc(chunk.len() as u64);
        }

        writer.shutdown().await?;

        info!("download finished");

        set_readonly(&cache).await?;

        let b3 = blake3(&cache).await?;
        let path = Artifact::storage_path(&b3)?;

        let download_len = metadata(&cache).await?.len();

        let len = match len {
            Some(len) if len != download_len => bail!(
                "download {} length mismatch, expected {len}",
                cache.display()
            ),
            Some(len) => len,
            None => download_len,
        };

        let mut art = Artifact::new(b3, name, Some(src), len);

        for checksum in checksums {
            if checksum.function == HashFunc::Blake3 {
                ensure!(
                    art.blake3 == checksum.hex_hash,
                    "blake3 mismatch for downloaded file"
                );
                continue;
            }

            if !checksum.check(&cache).await? {
                bail!("broken download {}, expected {checksum}", cache.display());
            }

            art.affix_checksum(checksum);
        }

        self.add_or_update(art.clone()).await?;

        if !self.has_storage(&art.blake3).await? {
            mv(&cache, &path).await?;
        }

        Ok(art)
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

    /// Parallel retrieve artifacts and create soft links.
    /// Each artifact is keyed by its relative path under the base path.
    ///
    /// See [`Self::retrieve_artifact_to`] for details and caveats.
    pub async fn batch_retrieve_artifact_to(
        &self,
        map: HashMap<PathBuf, Artifact>,
        base: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let base = base.as_ref();

        let count = stream::iter(map)
            .map(
                |(path, art)| async move { self.retrieve_artifact_to(&art, base.join(path)).await },
            )
            .buffer_unordered(self.config.parallel_download)
            .try_collect::<Vec<_>>()
            .await?
            .len();

        debug!("deployed {count} artifacts under {}", base.display());

        Ok(())
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

    /// Parallel download a batch of files keyed by `K` and store them in the artifact storage.
    /// Each file is described by a 4-tuple of `(name, src, len, checksum)`,
    /// as specified in [`Self::download`].
    pub async fn batch_download<K>(
        &self,
        download: HashMap<
            K,
            (
                String,
                String,
                Option<u64>,
                impl IntoIterator<Item = Checksum> + Send,
            ),
        >,
    ) -> anyhow::Result<HashMap<K, Artifact>>
    where
        K: Eq + Hash,
    {
        let map = stream::iter(download)
            .map(|(k, (name, src, len, checksum))| async move {
                self.download(name, src, len, checksum)
                    .await
                    .map(|a| (k, a))
            })
            .buffer_unordered(self.config.parallel_download)
            .try_collect::<HashMap<_, _>>()
            .await?;

        Ok(map)
    }

    /// Store a file to the artifact storage.
    ///
    /// # Note
    ///
    /// The file will not have a download source.
    pub async fn store_artifact(&self, file: impl AsRef<Path>) -> anyhow::Result<Artifact> {
        let file = file.as_ref();

        let b3 = blake3(file).await?;

        if let Some(art) = self.artifact.find(&b3).await? {
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

        self.artifact.add(&art).await?;

        Ok(art)
    }
}
