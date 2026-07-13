use std::{
    collections::HashMap,
    iter::once,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, ensure};
use tokio::{
    fs::{create_dir_all, read_link, read_to_string, remove_dir_all, try_exists, write},
    process::Command,
};

use crate::{Artifact, AssetIndex, Creeper, Install, symlink_auto};

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

        fn prefixed(
            map: &HashMap<PathBuf, Artifact>,
            prefix: &impl AsRef<Path>,
        ) -> impl Iterator<Item = String> {
            let prefix = prefix.as_ref();
            map.keys().map(|k| prefix.join(k).display().to_string())
        }

        let mut cp = vec![];

        cp.extend(prefixed(&install.java_lib_class, &lib_path));
        self.batch_retrieve_artifact_to(install.java_lib_class, &lib_path)
            .await?;

        if let Some(mc_jar) = install.mc_jar
            && !install.disable_mc_jar
        {
            let path = lib_path.join("minecraft.jar");
            self.retrieve_artifact_to(&mc_jar, &path).await?;
            cp.push(path.display().to_string());
        }

        let cp = cp.join(":");
        if !cp.is_empty() {
            cmd.arg("--class-path").arg(cp);
        }

        let mut p = vec![];

        p.extend(prefixed(&install.java_lib_mod, &lib_path));
        self.batch_retrieve_artifact_to(install.java_lib_mod, &lib_path)
            .await?;

        let p = p.join(":");
        if !p.is_empty() {
            cmd.arg("--module-path").arg(p);
        }

        self.batch_retrieve_artifact_to(install.java_lib_file, &lib_path)
            .await?;

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

        let mut assets = HashMap::new();

        for (_, art) in &install.mc_asset {
            let sha1 = art.sha1.as_ref().ok_or(anyhow!("missing SHA-1 checksum"))?;
            assets.insert(sha1_indexed_path(sha1)?, art.clone());
        }

        self.batch_retrieve_artifact_to(assets, asset_path.join("objects"))
            .await?;

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

        self.retrieve_ordered(&mod_dir, &install.mc_mod, Some("jar"))
            .await?;

        try_symlink(&mod_dir, self.game_mod_dir().await?).await?;

        let resource_dir = self.game_env_dir().await?.join("resource");

        if try_exists(&resource_dir).await? {
            remove_dir_all(&resource_dir).await?;
        }

        self.retrieve_ordered(&resource_dir, &install.resource_pack, Some("zip"))
            .await?;

        try_symlink(&resource_dir, self.game_resource_dir().await?).await?;

        let shader_dir = self.game_env_dir().await?.join("shader");

        if try_exists(&shader_dir).await? {
            remove_dir_all(&shader_dir).await?;
        }

        self.retrieve_ordered(&shader_dir, &install.shader_pack, Some("zip"))
            .await?;

        try_symlink(&shader_dir, self.game_shader_dir().await?).await?;

        Ok(cmd)
    }

    async fn retrieve_ordered(
        &self,
        dir: impl AsRef<Path>,
        art: impl IntoIterator<Item = &Artifact>,
        ext: Option<&str>,
    ) -> anyhow::Result<()> {
        let art = art.into_iter().collect::<Vec<_>>();

        let max_digit = art.len().to_string().len();

        let mut map = HashMap::new();

        for (idx, art) in art.into_iter().enumerate() {
            let file = format!("{idx:0max_digit$}");

            let path = PathBuf::from(file).with_added_extension(ext.unwrap_or(""));

            map.insert(path, art.clone());
        }

        create_dir_all(&dir).await?;

        self.batch_retrieve_artifact_to(map, &dir).await?;

        Ok(())
    }
}

async fn try_symlink(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> anyhow::Result<()> {
    let src = src.as_ref();
    let dst = dst.as_ref();

    if try_exists(dst).await? {
        if !dst.is_symlink() {
            bail!("{} not managed by creeper, please remove it", dst.display());
        }

        if read_link(dst).await? != src {
            bail!("{} not managed by creeper, please remove it", dst.display());
        }
    } else {
        symlink_auto(src, dst).await?;
    }

    Ok(())
}
