use std::{iter::once, path::PathBuf};

use anyhow::{anyhow, bail, ensure};
use tokio::{
    fs::{create_dir_all, read_link, read_to_string, remove_dir_all, symlink, try_exists, write},
    process::Command,
};

use crate::{Creeper, Install, vanilla::AssetIndex};

impl Creeper {
    pub async fn launch(&self) -> anyhow::Result<Command> {
        let game_dir = self.game_dir().await?;

        let json = read_to_string(self.game_env_dir().await?.join("install.json")).await?;

        let mut install = serde_json::from_str::<Install>(&json)?;

        install.extend(once(self.user_install().await?));

        let mut cmd = Command::new("java");

        cmd.current_dir(game_dir);

        for flag in install.java_flag {
            cmd.arg(flag);
        }

        let lib_path = self.game_env_dir().await?.join("lib");
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

        for (file, arg) in install.java_agent {
            let art = self.retrieve_artifact(&file).await?;

            let arg = if let Some(arg) = arg {
                format!("-javaagent:{}={arg}", art.display())
            } else {
                format!("-javaagent:{}", art.display())
            };

            cmd.arg(arg);
        }

        if let Some(java_main_class) = install.java_main_class {
            cmd.arg(java_main_class);
        }

        let asset_path = game_dir.join(".creeper").join("asset");
        create_dir_all(&asset_path).await?;

        fn sha1_indexed_path(sha1: &str) -> anyhow::Result<PathBuf> {
            ensure!(sha1.len() == 40, "invalid sha1 length");
            let first2 = &sha1[0..2];
            let path = PathBuf::from(".").join(first2).join(sha1);
            Ok(path)
        }

        for (_path, art) in &install.mc_asset {
            let sha1 = art.sha1.as_ref().ok_or(anyhow!("missing SHA-1 checksum"))?;
            let path = asset_path.join("objects").join(sha1_indexed_path(&sha1)?);

            self.retrieve_artifact_to(&art, &path).await?;
        }

        let asset_index = AssetIndex::from_map(install.mc_asset)?;

        let json = serde_json::to_string(&asset_index)?;
        let path = asset_path.join("indexes").join("index.json");
        create_dir_all(path.parent().unwrap()).await?;
        write(path, json).await?;

        cmd.arg("--assetsDir").arg(asset_path);
        cmd.arg("--assetIndex").arg("index");

        for flag in install.mc_flag {
            cmd.arg(flag);
        }

        cmd.arg("--gameDir").arg(game_dir);

        let mod_dir = game_dir.join(".creeper").join("mod");
        if try_exists(&mod_dir).await? {
            remove_dir_all(&mod_dir).await?;
        }
        create_dir_all(&mod_dir).await?;

        let count = install.mc_mod.len();
        let max_digit = count.to_string().len();

        for (idx, art) in install.mc_mod.into_iter().enumerate() {
            let file = format!("{:0width$}.jar", idx, width = max_digit);
            let path = mod_dir.join(file);
            self.retrieve_artifact_to(&art, &path).await?;
        }

        let game_mod_dir = self.game_mod_dir().await?;
        if try_exists(&game_mod_dir).await? {
            if !game_mod_dir.is_symlink() {
                bail!("mod directory not managed by creeper, please remove it");
            }
            if read_link(game_mod_dir).await? != mod_dir {
                bail!("mod directory not managed by creeper, please remove it");
            }
        } else {
            symlink(&mod_dir, &game_mod_dir).await?;
        }

        Ok(cmd)
    }
}
