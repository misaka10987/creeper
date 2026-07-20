use std::{collections::HashMap, path::PathBuf, time::Duration};

use anyhow::anyhow;
use neoforge::NfInstallProfile;
use reqwest::Client;
use semver::Version;
use tracing::{debug, info};

use crate::{
    Creeper, Id, Install, VersionRev,
    builtin::{SyncBuiltinIndex, UpdateIndex},
    index::{Index, independent_index},
    neoforge::{nf_required_mc_version, parse_neoforge_version, query_neoforge_versions},
    path::creeper_cache_dir,
    zip::{extract_zip, extract_zip_to},
};

pub struct NeoforgeServerManager {
    http: Client,
}

impl NeoforgeServerManager {
    pub fn new(http: Client) -> Self {
        Self { http }
    }
}

impl SyncBuiltinIndex for NeoforgeServerManager {
    fn package(&self) -> Id {
        Id::neoforge_server()
    }

    async fn sync_index(&self) -> anyhow::Result<Index> {
        let versions = query_neoforge_versions(&self.http).await?;

        let count = versions.len();

        let versions = versions
            .into_iter()
            .filter_map(|s| parse_neoforge_version(&s))
            .map(VersionRev::new);

        let index = independent_index(versions);

        debug!(
            "retrieved {count} NeoForge versions, of which {} valid",
            index.len()
        );

        Ok(index)
    }

    fn cache_expiry(&self) -> std::time::Duration {
        Duration::from_hours(72)
    }
}

impl Creeper {
    pub async fn update_neoforge_server(&self) -> anyhow::Result<()> {
        if self.args.offline {
            info!("skipping neoforge server update because offline mode enabled");
            return Ok(());
        }

        self.neoforge_server.update_index().await
    }

    pub(crate) async fn neoforge_server_install(
        &self,
        version: &Version,
    ) -> anyhow::Result<Install> {
        let installer = self.neoforge_installer_jar(version).await?;

        let installer = self.retrieve_artifact(&installer).await?;

        let mc_version = extract_zip(&installer, "version.json").await?;
        let mc_version = serde_json::from_str(&mc_version)?;

        let mut install = self.vanilla_version_install(mc_version).await?;

        let mut container =
            self.new_install_container(cache_path()?.join("tmp").join(version.to_string()));

        container.init().await?;

        let install_profile = extract_zip(&installer, "install_profile.json").await?;

        let install_profile = serde_json::from_str::<NfInstallProfile>(&install_profile)?;

        let mut java_lib_file = self.vanilla_lib(install_profile.libraries).await?;

        container.add_lib_file(java_lib_file.clone());

        let vanilla_install = {
            // repeat code from [`Self::install`] to avoid async recursion
            let version = nf_required_mc_version(version);
            if let Some(install) = self
                .get_install_cache(&Id::server(), &version.clone().into())
                .await?
            {
                install
            } else {
                let install = self.server_install(&version).await?;
                self.set_install_cache(&Id::server(), &version.into(), Some(&install))
                    .await?;
                install
            }
        };

        let mc_jar = vanilla_install
            .mc_jar
            .ok_or(anyhow!("missing minecraft jar in vanilla install"))?;
        let mc_jar = self.retrieve_artifact(&mc_jar).await?;

        let mut vars = install_profile
            .data
            .into_iter()
            .map(|(k, v)| (k, v.client))
            .chain([
                ("SIDE".into(), "server".into()),
                ("MINECRAFT_JAR".into(), mc_jar.display().to_string()),
                ("ROOT".into(), container.path().display().to_string()),
                ("INSTALLER".into(), installer.display().to_string()),
            ])
            .collect::<HashMap<_, _>>();

        let binpatch = container
            .path()
            .join(".installer")
            .join("data")
            .join("server.lzma");
        extract_zip_to(&installer, "data/server.lzma", &binpatch).await?;
        vars.insert("BINPATCH".into(), binpatch.display().to_string());

        container.add_var(vars);
        container.deploy_lib().await?;

        for proc in install_profile.processors {
            if !proc
                .sides
                .as_ref()
                .is_none_or(|x| x.contains(&"server".into()))
            {
                debug!("skipping a processor because side mismatch: {proc}");
                continue;
            }

            container.run(&proc).await?;
        }

        todo!()
    }
}

fn cache_path() -> anyhow::Result<PathBuf> {
    let path = creeper_cache_dir()?.join("builtin").join("neoforge-server");

    Ok(path)
}
