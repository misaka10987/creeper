use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::bail;
use base64::{Engine, prelude::BASE64_URL_SAFE};
use inquire::{Confirm, Text};
use semver::{Version, VersionReq};
use serde::{Serialize, de::DeserializeOwned};
use tokio::{
    fs::{
        File, copy, create_dir_all, metadata, read_to_string, remove_file, rename, set_permissions,
        try_exists, write,
    },
    sync::RwLock,
    task::spawn_blocking,
};
use tracing::trace;

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

pub async fn set_readonly(path: impl AsRef<Path>) -> anyhow::Result<()> {
    let path = path.as_ref();

    let metadata = metadata(path).await?;

    let mut perm = metadata.permissions();
    perm.set_readonly(true);

    set_permissions(path, perm).await?;

    trace!("set {} to readonly", path.display());

    Ok(())
}

pub struct TomlFile<T>
where
    T: Clone + Serialize + DeserializeOwned,
{
    cache: RwLock<OnceLock<Option<T>>>,
}

impl<T> TomlFile<T>
where
    T: Clone + Serialize + DeserializeOwned,
{
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(OnceLock::new()),
        }
    }

    pub async fn read(&self, path: impl AsRef<Path>) -> anyhow::Result<Option<T>> {
        if let Some(value) = self.cache.read().await.get() {
            return Ok(value.clone());
        }

        let value = if try_exists(&path).await? {
            let toml = read_to_string(&path).await?;
            Some(toml::from_str(&toml)?)
        } else {
            None
        };

        let value = self.cache.write().await.get_or_init(|| value).clone();

        Ok(value)
    }

    pub async fn write(&self, path: impl AsRef<Path>, value: Option<T>) -> anyhow::Result<()> {
        let path = path.as_ref();

        *self.cache.write().await = value.clone().into();

        if let Some(value) = value {
            let toml = toml::to_string(&value)?;

            if let Some(parent) = path.parent() {
                create_dir_all(parent).await?;
            }

            write(path, toml).await?;
        } else {
            if try_exists(path).await? {
                remove_file(path).await?;
            }
        }

        Ok(())
    }
}

pub async fn prompt_save(content: impl AsRef<[u8]>, path: impl AsRef<Path>) -> anyhow::Result<()> {
    let content = content.as_ref();
    let path = path.as_ref();

    let message = format!("Save {} bytes to file?", content.len());

    let confirm =
        spawn_blocking(move || Confirm::new(&message).with_default(false).prompt()).await??;

    if !confirm {
        return Ok(());
    }

    let default = path.display().to_string();

    let path = spawn_blocking(move || {
        Text::new("Enter the path to save to")
            .with_default(&default)
            .prompt()
    })
    .await??;

    let path = PathBuf::from(path);

    if let Some(parent) = path.parent() {
        create_dir_all(parent).await?;
    }

    write(&path, content).await?;

    Ok(())
}

pub async fn symlink_auto(
    original: impl AsRef<Path>,
    link: impl AsRef<Path>,
) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use tokio::fs::symlink;

        symlink(original, link).await?;

        Ok(())
    }

    #[cfg(windows)]
    {
        use tokio::fs::{symlink_dir, symlink_file};

        let original = original.as_ref();

        if !try_exists(original).await? {
            bail!(
                "cannot create symlink on windows: original path {} does not exist",
                original.display()
            );
        }

        let meta = metadata(original).await?;

        if meta.is_dir() {
            symlink_dir(original, link).await?;
        } else if meta.is_file() {
            symlink_file(original, link).await?;
        } else {
            panic!();
        }

        Ok(())
    }
}

pub fn rebuild_req(
    versions: BTreeSet<Version>,
    univ: BTreeSet<Version>,
) -> anyhow::Result<VersionReq> {
    if !versions.is_subset(&univ) {
        bail!("versions not subset of universe");
    }

    if versions.is_empty() {
        // empty set
        let req = format!("<1.0.0, >=1.0.0").parse().unwrap();
        return Ok(req);
    }

    let start = versions.first().unwrap();

    let end = univ.range(start..).find(|v| !versions.contains(v));

    let end = match end {
        Some(v) => v,
        None => {
            let end = univ.last().unwrap();
            return Ok(format!(">={start}, <={end}",).parse().unwrap());
        }
    };

    if end < versions.last().unwrap() {
        bail!("versions contains a gap");
    }

    let req = format!(">={start}, <{end}").parse().unwrap();

    Ok(req)
}

/// Like [`Iterator::filter`], but it also immediately skips the next element after a match.
///
/// Also note that that an element is skipped when `skip` returns `true`, negation of [`Iterator::filter`].
pub fn skip_two<T>(skip: impl Fn(&T) -> bool, it: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut keep = vec![];

    let mut it = it.into_iter();

    while let Some(x) = it.next() {
        if skip(&x) {
            it.next();
            continue;
        }

        keep.push(x);
    }

    keep
}

/// Summarize a string into a shorter valid filename.
///
/// While hashing a string directly also feasible for the purpose of generating a filename,
/// this function provides a (partially) invertible summary of the string,
/// so that the original string remains still recognizable.
///
/// # Format
///
/// 8 characters of hexadecimal blake3 hash of string followed by first 64 characters of base64-url-safe encoded string,
/// separated by a dash `-`.
pub fn summarize(name: &str) -> String {
    let hash = blake3::hash(name.as_bytes()).to_hex();

    let base64 = BASE64_URL_SAFE.encode(name);

    format!("{}-{}", &hash[..8], &base64[..64.min(base64.len())])
}
