use std::path::Path;

use anyhow::{bail, ensure};
use bytesize::ByteSize;
use reqwest::{IntoUrl, header::CONTENT_TYPE};
use tokio::{
    fs::{File, create_dir_all, metadata, remove_file, try_exists},
    io::{AsyncWriteExt, BufWriter},
};
use tracing::{Span, debug, info, instrument, trace, warn};
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::{
    Artifact, Checksum,
    artifact::ArtifactManager,
    checksum::{HashFunc, blake3},
    mv,
    path::creeper_cache_dir,
    pbar::PROGRESS_STYLE_DOWNLOAD,
    singleflight::SingleFlightGuard,
    util::{set_readonly, summarize},
};

impl ArtifactManager {
    /// If the fingerprint is found in the artifact database,
    /// verify its checksum and return the artifact without download.
    async fn skip_download(&self, checksum: Vec<Checksum>) -> anyhow::Result<Option<Artifact>> {
        for sum in &checksum {
            if let Some(mut art) = self.get_checksum(sum).await? {
                debug!("fingerprint {sum} found in local storage");

                let path = self.retrieve(&art).await?;

                trace!("checking retrieved artifact at {}", path.display());

                let func = sum.function;

                for checksum in checksum {
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

                trace!("download can be skipped");

                return Ok(Some(art));
            }
        }

        Ok(None)
    }

    /// Download a file from the supplied URL and save it to the specified path.
    ///
    /// Low-level function. No single-flight, checksum verification, resumable download or any artifact database operation.
    pub(super) async fn download_file(
        &self,
        name: &str,
        len: Option<u64>,
        src: impl IntoUrl,
        dst: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let src = src.into_url()?;
        let dst = dst.as_ref();

        let size = if let Some(size) = len {
            ByteSize::b(size).to_string()
        } else {
            "(unknown size)".into()
        };

        debug!("downloading {} from {src} to {}", size, dst.display());

        if let Some(parent) = dst.parent() {
            create_dir_all(parent).await?;
        }

        let file = File::create(dst).await?;

        let mut writer = BufWriter::new(file);

        let http = self.http.get().await;

        trace!("sending HTTP request");

        let mut res = http.get(src).send().await?.error_for_status()?;

        let content_type = if let Some(header) = res.headers().get(CONTENT_TYPE) {
            format!("Content-Type: {}", header.to_str()?)
        } else {
            "unknown Content-Type".into()
        };

        trace!("received HTTP response with {content_type}");

        let span = Span::current();

        span.pb_set_message(name);
        span.pb_set_style(&PROGRESS_STYLE_DOWNLOAD);
        span.pb_set_length(len.or(res.content_length()).unwrap_or(0));

        while let Some(chunk) = res.chunk().await? {
            writer.write_all(&chunk).await?;
            span.pb_inc(chunk.len() as u64);
        }

        writer.shutdown().await?;

        Ok(())
    }

    async fn try_skip_download(
        &self,
        src: String,
        checksum: Vec<Checksum>,
    ) -> anyhow::Result<Result<Artifact, SingleFlightGuard<'_>>> {
        if let Some(art) = self.skip_download(checksum.clone()).await? {
            trace!("found {art} matching fingerprint");

            return Ok(Ok(art));
        }

        let mut queue = self.single_flight.queue(src);

        let single_flight = loop {
            let advance = queue.advance().await;

            if let Some(art) = self.skip_download(checksum.clone()).await? {
                trace!("found {art} matching fingerprint");

                return Ok(Ok(art));
            }

            if let Some(x) = advance {
                break x;
            }
        };

        trace!("single-flight lock acquired for download");

        Ok(Err(single_flight))
    }

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

        let single_flight = match self.try_skip_download(src, checksums.clone()).await? {
            Ok(art) => {
                debug!("skipped download");
                return Ok(art);
            }
            Err(x) => x,
        };

        let src = &*single_flight;

        if self.offline {
            bail!("offline mode enabled, cannot download {src}");
        }

        let cache = creeper_cache_dir()?.join("download").join(summarize(&src));

        trace!("download caching to {cache:?}");
        create_dir_all(cache.parent().unwrap()).await?;

        if try_exists(&cache).await? {
            warn!("TODO: continue download if incomplete instead of removing partial content");
            remove_file(&cache).await?;
        }

        self.download_file(&name, len, src, &cache).await?;

        info!("download finished");

        set_readonly(&cache).await?;

        let art = check_download_file(&cache, name, src.clone(), len, checksums).await?;

        self.add_or_update(art.clone()).await?;

        if !self.has_storage(&art.blake3).await? {
            mv(&cache, art.path()?).await?;
        } else {
            warn!("unnessary download detected");
            remove_file(&cache).await?;
        }

        single_flight.release();

        Ok(art)
    }
}

async fn check_download_file(
    file: impl AsRef<Path>,
    name: String,
    src: String,
    len: Option<u64>,
    checksum: Vec<Checksum>,
) -> anyhow::Result<Artifact> {
    let file = file.as_ref();

    let b3 = blake3(file).await?;

    let download_len = metadata(file).await?.len();

    let len = match len {
        Some(len) if len != download_len => bail!(
            "download {} length mismatch: expected {len}, found {download_len}",
            file.display()
        ),
        Some(len) => len,
        None => download_len,
    };

    let mut art = Artifact::new(b3, name, Some(src), len);

    for c in checksum {
        if c.function == HashFunc::Blake3 {
            ensure!(
                art.blake3 == c.hex_hash,
                "blake3 mismatch for downloaded file"
            );
            continue;
        }

        if !c.check(file).await? {
            bail!("broken download {}, expected {c}", file.display());
        }

        art.affix_checksum(c);
    }

    Ok(art)
}
