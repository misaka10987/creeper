use tokio::{fs::read_to_string, process::Command};

use crate::{Creeper, Install};

impl Creeper {
    pub async fn launch(&self) -> anyhow::Result<Command> {
        let path = self.game_dir().await?.join(".creeper").join("install.json");
        let json = read_to_string(path).await?;

        let install = serde_json::from_str::<Install>(&json)?;

        let mut cmd = Command::new("java");

        for flag in install.java_flag {
            cmd.arg(flag);
        }

        let mut cp = vec![];

        for lib in install.java_lib {
            let art = self.retrieve_artifact(&lib).await?;
            cp.push(art.display().to_string());
        }

        if let Some(mc_jar) = install.mc_jar {
            let art = self.retrieve_artifact(&mc_jar).await?;
            cp.push(art.display().to_string());
        }

        let cp = cp.join(":");
        cmd.arg("-cp").arg(cp);

        if let Some(java_main_class) = install.java_main_class {
            cmd.arg(java_main_class);
        }

        for flag in install.mc_flag {
            cmd.arg(flag);
        }

        Ok(cmd)
    }
}
