use anyhow::{bail, ensure};
use base64::{Engine, prelude::BASE64_URL_SAFE};
use tokio::{
    fs::{File, create_dir_all, metadata, remove_file, try_exists},
    io::{AsyncWriteExt, BufWriter},
};
use tracing::{Span, debug, info, instrument, trace};
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::{
    Artifact, Checksum,
    artifact::ArtifactManager,
    checksum::{HashFunc, blake3},
    mv,
    path::creeper_cache_dir,
    pbar::PROGRESS_STYLE_DOWNLOAD,
    util::set_readonly,
};

impl ArtifactManager {
    /// See [`Creeper::download`].
    #[instrument(skip(self, name, len, checksum))]
    pub(super) async fn download(
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
            if let Some(mut art) = self.get_checksum(checksum).await? {
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

        let semaphore = self.semaphore.acquire().await?;

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

        drop(semaphore);

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
