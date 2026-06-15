use std::path::Path;

use anyhow::anyhow;
use async_zip::base::read::seek::ZipFileReader;
use tokio::{
    fs::{File, create_dir_all},
    io::{BufReader, copy},
};
use tokio_util::compat::FuturesAsyncReadCompatExt;

/// Extract a text file from a zip archive `zip_file` at the path `path`.
///
/// # Panics
///
/// The function panics unless `path` is valid UTF-8.
pub async fn extract_zip(
    zip_file: impl AsRef<Path>,
    path: impl AsRef<Path>,
) -> anyhow::Result<String> {
    let zip_file = zip_file.as_ref();
    let path = path.as_ref();

    let zip = File::open(&zip_file).await?;
    let read = BufReader::new(zip);

    let mut zip = ZipFileReader::with_tokio(read).await?;

    let idx = zip
        .file()
        .entries()
        .iter()
        .position(|e| {
            e.filename()
                .as_str()
                .is_ok_and(|s| s == path.to_str().unwrap())
        })
        .ok_or(anyhow!(
            "{} not found in {}",
            path.display(),
            zip_file.display()
        ))?;

    let mut read = zip.reader_with_entry(idx).await?;

    let mut buf = String::new();
    read.read_to_string_checked(&mut buf).await?;

    Ok(buf)
}

/// Extract a text file from a zip archive `zip_file` at the path `path` and save it to `dst`.
/// Creating parent directories if necessary.
///
/// # Panics
///
/// The function panics unless `path` is valid UTF-8.
pub async fn extract_zip_to(
    zip_file: impl AsRef<Path>,
    path: impl AsRef<Path>,
    dst: impl AsRef<Path>,
) -> anyhow::Result<()> {
    let zip_file = zip_file.as_ref();
    let path = path.as_ref();

    let zip = File::open(&zip_file).await?;
    let read = BufReader::new(zip);

    let mut zip = ZipFileReader::with_tokio(read).await?;

    let idx = zip
        .file()
        .entries()
        .iter()
        .position(|e| {
            e.filename()
                .as_str()
                .is_ok_and(|s| s == path.to_str().unwrap())
        })
        .ok_or(anyhow!(
            "{} not found in {}",
            path.display(),
            zip_file.display()
        ))?;

    let mut read = zip.reader_with_entry(idx).await?.compat();

    if let Some(parent) = dst.as_ref().parent() {
        create_dir_all(parent).await?;
    }

    copy(&mut read, &mut File::create(dst).await?).await?;

    Ok(())
}
