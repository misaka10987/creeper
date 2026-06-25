use std::{collections::HashMap, path::Path, str::FromStr, sync::OnceLock};

use anyhow::{anyhow, bail};
use inquire::Confirm;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::{
    fs::{
        File, copy, create_dir_all, metadata, read_to_string, remove_dir_all, remove_file, rename,
        set_permissions, try_exists, write,
    },
    sync::RwLock,
};
use tracing::info;

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

    Ok(())
}

/// Parse the first section of an RFC 822-like format.
///
/// # Note
///
/// TODO: this function does not yet guarantee complete support for the RFC 822 and there may exist behavioral difference in edge cases.
pub fn rfc822_first_section(s: &str) -> anyhow::Result<HashMap<&str, &str>> {
    let mut map = HashMap::new();

    let lines = s.lines().take_while(|l| !l.is_empty());

    for line in lines {
        let (key, value) = line.split_once(": ").ok_or(anyhow!("invalid line"))?;
        map.insert(key, value);
    }

    Ok(map)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct JarManifest {
    pub manifest_version: String,
    pub implementation_version: Option<String>,
    pub main_class: Option<String>,
}

impl FromStr for JarManifest {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let map = rfc822_first_section(s)?;

        let manifest_version = map
            .get("Manifest-Version")
            .ok_or(anyhow!("missing field Manifest-Version"))?
            .to_string();
        let implementation_version = map.get("Implementation-Version").map(|s| s.to_string());
        let main_class = map.get("Main-Class").map(|s| s.to_string());

        Ok(Self {
            manifest_version,
            implementation_version,
            main_class,
        })
    }
}

/// Prompt the user to confirm the removal of a file or directory, and remove it if confirmed.
pub async fn prompt_remove(path: impl AsRef<Path>) -> anyhow::Result<()> {
    let path = path.as_ref();
    let confirm = Confirm::new(&format!("Remove {}?", path.display())).prompt()?;
    if !confirm {
        bail!("aborted by user");
    }
    info!("removing {}", path.display());
    remove_dir_all(path).await?;
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
