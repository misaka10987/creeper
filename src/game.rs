use std::{
    env::current_dir,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::anyhow;
use tokio::{
    fs::{create_dir_all, read_to_string, remove_file, try_exists, write},
    sync::RwLock,
};

use crate::{Creeper, Package, lock::Lock};

pub struct GameManager {
    dir: OnceLock<PathBuf>,
    pack: OnceLock<Package>,
    lock: RwLock<OnceLock<Option<Lock>>>,
}

impl GameManager {
    pub fn new(dir: Option<PathBuf>) -> Self {
        let d = OnceLock::new();
        if let Some(dir) = dir {
            d.set(dir).unwrap();
        }
        Self {
            dir: d,
            pack: OnceLock::new(),
            lock: RwLock::new(OnceLock::new()),
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

    pub async fn pack(&self) -> anyhow::Result<&Package> {
        if let Some(pack) = self.pack.get() {
            return Ok(pack);
        }

        let toml = read_to_string(self.pack_path().await?).await?;
        let pack = toml::from_str(&toml)?;
        Ok(self.pack.get_or_init(|| pack))
    }

    pub async fn lock(&self) -> anyhow::Result<Option<Lock>> {
        if let Some(lock) = self.lock.read().await.get() {
            return Ok(lock.clone());
        }

        let path = self.lock_path().await?;

        let lock = if try_exists(path).await? {
            let toml = read_to_string(self.lock_path().await?).await?;
            Some(toml::from_str(&toml)?)
        } else {
            None
        };

        let lock = self.lock.write().await.get_or_init(|| lock).clone();

        Ok(lock)
    }

    pub async fn set_lock(&self, lock: Option<Lock>) -> anyhow::Result<()> {
        *self.lock.write().await = lock.clone().into();

        let path = self.lock_path().await?;

        if let Some(lock) = lock {
            let toml = toml::to_string(&lock)?;
            create_dir_all(path.parent().unwrap()).await?;
            write(&path, toml).await?;
        } else {
            if try_exists(&path).await? {
                remove_file(&path).await?;
            }
        }

        Ok(())
    }
}

impl Creeper {
    pub async fn game_dir(&self) -> anyhow::Result<&PathBuf> {
        self.game.dir().await
    }

    pub async fn game_pack(&self) -> anyhow::Result<&Package> {
        self.game.pack().await
    }

    pub async fn game_lock(&self) -> anyhow::Result<Option<Lock>> {
        self.game.lock().await
    }

    pub async fn set_game_lock(&self, lock: Option<Lock>) -> anyhow::Result<()> {
        self.game.set_lock(lock).await
    }
}

// impl LaunchOption for InstConfig {
//     fn java_flags(&self) -> Vec<String> {
//         let mut flags = vec![];

//         // common flags
//         flags.extend([
//             "-Dfile.encoding=UTF-8".into(),
//             "-Dstdout.encoding=UTF-8".into(),
//             "-Dstderr.encoding=UTF-8".into(),
//             "-Duser.home=null".into(),
//             "-Djava.rmi.server.useCodebaseOnly=true".into(),
//             "-Dcom.sun.jndi.rmi.object.trustURLCodebase=false".into(),
//             "-Dcom.sun.jndi.cosnaming.object.trustURLCodebase=false".into(),
//             "-Dlog4j2.formatMsgNoLookups=true".into(),
//         ]);

//         flags.extend([
//             format!(
//                 "-Dlog4j.configurationFile={}",
//                 self.dir.join("log4j2.xml").display()
//             ),
//             format!(
//                 "-Dminecraft.client.jar={}",
//                 self.dir.join("minecraft.jar").display()
//             ),
//         ]);

//         // LWJGL path
//         flags.push(format!("-Djava.library.path={}", self.mc.lwjgl.display()));

//         // launcher identifiers for java runtime
//         flags.extend([
//             "-Dminecraft.launcher.brand=creeper".into(),
//             format!("-Dminecraft.launcher.version={}", VERSION),
//         ]);

//         // class paths
//         let mut cp = self
//             .mc
//             .java_libs
//             .iter()
//             .map(|p| p.display().to_string())
//             .collect::<Vec<_>>();
//         cp.push(self.dir.join("minecraft.jar").display().to_string());
//         flags.extend(["-cp".into(), cp.join(":")]);

//         flags
//     }

//     fn game_flags(&self) -> Vec<String> {
//         let mut flags = vec![];

//         flags.extend(["--version".into(), self.name.clone()]);

//         // launcher identifiers for game
//         flags.extend(["--versionType".into(), format!("creeper {}", VERSION)]);

//         // dirs
//         flags.extend([
//             "--gameDir".into(),
//             self.dir.display().to_string(),
//             "--assetsDir".into(),
//             self.mc.asset.display().to_string(),
//         ]);

//         // window size
//         flags.extend([
//             "--width".into(),
//             format!("{}", self.mc.width),
//             "--height".into(),
//             format!("{}", self.mc.height),
//         ]);

//         flags.extend(self.user.game_flags());

//         flags.extend(self.mc.game_flags.clone());

//         flags
//     }
// }
