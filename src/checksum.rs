use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use ring::digest::{Context, SHA1_FOR_LEGACY_USE_ONLY};
use tokio::task::spawn_blocking;

fn file_sha1_impl(path: impl AsRef<Path>) -> anyhow::Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut ctx = Context::new(&SHA1_FOR_LEGACY_USE_ONLY);
    let mut buf = [0u8; 4096];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        ctx.update(&buf[..n]);
    }
    let digest = ctx.finish();
    let hex = const_hex::encode(digest.as_ref());
    Ok(hex)
}

pub async fn file_sha1(path: impl AsRef<Path>) -> anyhow::Result<String> {
    let path = path.as_ref().to_owned();
    spawn_blocking(|| file_sha1_impl(path)).await?
}
