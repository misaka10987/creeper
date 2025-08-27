use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use tokio::fs::read_to_string;

use crate::{Java, User};

/// Defines a game instance.
///
/// This is stored in `creeper.toml`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Inst {
    /// Name for this instance.
    ///
    /// This is used by the `INST_NAME` variable passed to the game.
    pub name: String,

    pub user: User,

    pub java: Java,

    /// Minecraft configuration.
    #[serde(rename = "minecraft")]
    pub mc: MCConfig,
}

impl Inst {
    pub async fn load(dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let dir = dir.as_ref();
        let toml = read_to_string(dir.join("creeper.toml")).await?;
        let val = toml::from_str(&toml)?;
        Ok(val)
    }

    /// Given a certain path, retrive the game instance it belongs to.
    pub fn find_dir(start: impl AsRef<Path>) -> Option<PathBuf> {
        let curr = start.as_ref();
        if curr.join("creeper.toml").exists() {
            return Some(curr.into());
        }
        curr.parent().and_then(Self::find_dir)
    }
}

#[serde_inline_default]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct MCConfig {
    /// Additional flags passed to the game.
    #[serde(default)]
    pub game_flags: Vec<String>,

    /// Initial window width.
    #[serde_inline_default(854)]
    pub width: i32,

    /// Initial window height.
    #[serde_inline_default(480)]
    pub height: i32,
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
