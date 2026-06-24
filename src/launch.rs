use tokio::{
    fs::{create_dir_all, read_to_string},
    process::Command,
};

use crate::{Creeper, Install};

impl Creeper {
    pub async fn launch(&self) -> anyhow::Result<Command> {
        let game_dir = self.game_dir().await?;

        let path = game_dir.join(".creeper").join("install.json");
        let json = read_to_string(path).await?;

        let install = serde_json::from_str::<Install>(&json)?;

        let mut cmd = Command::new("java");

        cmd.current_dir(game_dir);

        for flag in install.java_flag {
            cmd.arg(flag);
        }

        let lib_path = game_dir.join(".creeper").join("lib");
        create_dir_all(&lib_path).await?;

        let mut cp = vec![];

        for (path, art) in install.java_lib_class {
            let path = lib_path.join(path);
            self.retrieve_artifact_to(&art, &path).await?;
            cp.push(path.display().to_string());
        }

        if let Some(mc_jar) = install.mc_jar
            && !install.disable_mc_jar
        {
            let art = self.retrieve_artifact(&mc_jar).await?;
            cp.push(art.display().to_string());
        }

        let cp = cp.join(":");
        if !cp.is_empty() {
            cmd.arg("--class-path").arg(cp);
        }

        let mut p = vec![];

        for (path, art) in install.java_lib_mod {
            let path = lib_path.join(path);
            self.retrieve_artifact_to(&art, &path).await?;
            p.push(path.display().to_string());
        }

        let p = p.join(":");
        if !p.is_empty() {
            cmd.arg("--module-path").arg(p);
        }

        for (path, art) in install.java_lib_file {
            let path = lib_path.join(path);
            self.retrieve_artifact_to(&art, &path).await?;
        }

        if let Some(java_main_class) = install.java_main_class {
            cmd.arg(java_main_class);
        }

        for flag in install.mc_flag {
            cmd.arg(flag);
        }

        cmd.arg("--gameDir").arg(game_dir);

        Ok(cmd)
    }
}
