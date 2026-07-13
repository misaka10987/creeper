use std::{
    env::current_dir,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::anyhow;
use tokio::fs::try_exists;

use crate::{Creeper, Package, lock::Lock, util::TomlFile};

pub struct GameManager {
    dir: OnceLock<PathBuf>,
    pack: TomlFile<Package>,
    lock: TomlFile<Lock>,
}

impl GameManager {
    pub fn new(dir: Option<PathBuf>) -> Self {
        let d = OnceLock::new();
        if let Some(dir) = dir {
            d.set(dir).unwrap();
        }
        Self {
            dir: d,
            pack: TomlFile::new(),
            lock: TomlFile::new(),
        }
    }

    async fn find_dir(start: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
        let mut curr = start.as_ref().to_path_buf();
        loop {
            if try_exists(curr.join("creeper.toml")).await? {
                break Ok(curr);
            }
            let parent = curr.parent().ok_or(anyhow!("not in any game instance"))?;
            curr = parent.into();
        }
    }

    pub async fn dir(&self) -> anyhow::Result<&PathBuf> {
        if let Some(dir) = self.dir.get() {
            return Ok(dir);
        }

        let found = Self::find_dir(current_dir()?).await?;
        Ok(self.dir.get_or_init(|| found))
    }

    pub async fn pack_path(&self) -> anyhow::Result<PathBuf> {
        let dir = self.dir().await?;
        Ok(dir.join("creeper.toml"))
    }

    pub async fn lock_path(&self) -> anyhow::Result<PathBuf> {
        let dir = self.dir().await?;
        Ok(dir.join("creeper.lock"))
    }

    pub async fn pack(&self) -> anyhow::Result<Package> {
        let path = self.pack_path().await?;

        let pack = self
            .pack
            .read(path)
            .await?
            .ok_or(anyhow!("missing creeper.toml"))?;

        Ok(pack)
    }

    pub async fn set_pack(&self, pack: Package) -> anyhow::Result<()> {
        let path = self.pack_path().await?;

        self.pack.write(path, Some(pack)).await?;

        Ok(())
    }

    pub async fn lock(&self) -> anyhow::Result<Option<Lock>> {
        let path = self.lock_path().await?;

        let lock = self.lock.read(path).await?;

        Ok(lock)
    }

    pub async fn set_lock(&self, lock: Option<Lock>) -> anyhow::Result<()> {
        let path = self.lock_path().await?;

        self.lock.write(path, lock).await?;

        Ok(())
    }
}

impl Creeper {
    pub async fn game_dir(&self) -> anyhow::Result<&PathBuf> {
        self.game.dir().await
    }

    pub async fn game_env_dir(&self) -> anyhow::Result<PathBuf> {
        let dir = self.game_dir().await?.join(".creeper");
        Ok(dir)
    }

    pub async fn game_mod_dir(&self) -> anyhow::Result<PathBuf> {
        let dir = self.game_dir().await?.join("mods");
        Ok(dir)
    }

    pub async fn game_resource_dir(&self) -> anyhow::Result<PathBuf> {
        let dir = self.game_dir().await?.join("resourcepacks");
        Ok(dir)
    }

    pub async fn game_shader_dir(&self) -> anyhow::Result<PathBuf> {
        let dir = self.game_dir().await?.join("shaderpacks");
        Ok(dir)
    }

    pub async fn game_pack(&self) -> anyhow::Result<Package> {
        self.game.pack().await
    }

    pub async fn set_game_pack(&self, pack: Package) -> anyhow::Result<()> {
        self.game.set_pack(pack).await
    }

    pub async fn game_lock(&self) -> anyhow::Result<Option<Lock>> {
        self.game.lock().await
    }

    pub async fn set_game_lock(&self, lock: Option<Lock>) -> anyhow::Result<()> {
        self.game.set_lock(lock).await
    }
}
