use tokio::{
    fs::{create_dir_all, read_to_string, symlink, try_exists},
    process::Command,
};

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

        let lib_path = self.game_dir().await?.join(".creeper").join("lib");
        create_dir_all(&lib_path).await?;

        let mut cp = vec![];

        for (path, art) in install.java_lib_class {
            let path = lib_path.join(path);
            create_dir_all(path.parent().unwrap()).await?;

            if !(try_exists(&path).await? && art.verify(&path).await?) {
                let art = self.retrieve_artifact(&art).await?;
                symlink(&art, &path).await?;
            }

            cp.push(path.display().to_string());
        }

        if let Some(mc_jar) = install.mc_jar {
            let art = self.retrieve_artifact(&mc_jar).await?;
            cp.push(art.display().to_string());
        }

        let cp = cp.join(":");
        cmd.arg("--class-path").arg(cp);

        let mut p = vec![];

        for (path, art) in install.java_lib_mod {
            let path = lib_path.join(path);
            create_dir_all(path.parent().unwrap()).await?;

            if !(try_exists(&path).await? && art.verify(&path).await?) {
                let art = self.retrieve_artifact(&art).await?;
                symlink(&art, &path).await?;
            }

            p.push(path.display().to_string());
        }

        for (path, art) in install.java_lib_file {
            let path = lib_path.join(path);
            create_dir_all(path.parent().unwrap()).await?;

            if !(try_exists(&path).await? && art.verify(&path).await?) {
                let art = self.retrieve_artifact(&art).await?;
                symlink(&art, &path).await?;
            }
        }

        let p = p.join(":");
        cmd.arg("--module-path").arg(p);

        cmd.arg(format!("-DlibraryDirectory={}", lib_path.display()));

        if let Some(java_main_class) = install.java_main_class {
            cmd.arg(java_main_class);
        }

        for flag in install.mc_flag {
            cmd.arg(flag);
        }

        cmd.arg("--gameDir").arg(self.game_dir().await?);

        Ok(cmd)
    }
}
