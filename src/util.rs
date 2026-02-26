use std::path::Path;

use tokio::fs::{File, copy, create_dir_all, remove_file, rename};

pub async fn mv(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> anyhow::Result<()> {
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
