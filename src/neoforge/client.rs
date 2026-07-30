use std::{collections::HashMap, iter::once, path::PathBuf, time::Duration};

use anyhow::anyhow;
use neoforge::NfInstallProfile;
use semver::Version;
use tracing::{debug, info};

use crate::{
    Creeper, Id, Install,
    builtin::{SyncBuiltinIndex, neoforge_client_id, vanilla_id},
    http::HttpThrottle,
    index::Index,
    neoforge::{NfVersion, neoforge_index, nf_mc_req, query_neoforge_versions},
    path::creeper_cache_dir,
    zip::{extract_zip, extract_zip_to},
};

pub struct NeoforgeClientManager {
    http: HttpThrottle,
}

impl NeoforgeClientManager {
    pub fn new(http: HttpThrottle) -> Self {
        Self { http }
    }
}

impl SyncBuiltinIndex for NeoforgeClientManager {
    fn package(&self) -> Id {
        neoforge_client_id()
    }

    async fn sync_index(&self) -> anyhow::Result<Index> {
        info!("updating NeoForge metadata");

        let versions = query_neoforge_versions(&self.http).await?;

        let count = versions.len();

        let versions = versions
            .into_iter()
            .filter_map(|s| s.parse::<NfVersion>().ok())
            .filter_map(|v| v.encode().ok());

        let index = neoforge_index(versions);

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
    pub(crate) async fn neoforge_client_install(
        &self,
        version: Version,
    ) -> anyhow::Result<Install> {
        let installer = self.neoforge_installer_jar(version.clone()).await?;

        let installer = self.retrieve_artifact(&installer).await?;

        // handle install as defined in `version.json`

        let mc_version = extract_zip(&installer, "version.json").await?;
        let mc_version = serde_json::from_str(&mc_version)?;

        let mut install = self.mc_version_install(mc_version).await?;

        // handle install as defined in `install_profile.json`

        let mut container =
            self.new_install_container(cache_path()?.join("tmp").join(version.to_string()));
        container.init().await?;

        let install_profile = extract_zip(&installer, "install_profile.json").await?;
        let install_profile = serde_json::from_str::<NfInstallProfile>(&install_profile)?;

        // libraries defined in `install_profile.json` does not require being prepended to `--module-path`
        // because they are loaded by neoforge's custom class loader
        let mut java_lib_file = self.vanilla_lib(install_profile.libraries).await?;

        container.add_lib_file(java_lib_file.clone());

        info!("preparing neoforge install environment");

        let vanilla_install = {
            // repeat code from [`Self::install`] to avoid async recursion
            let version = nf_mc_req(&version);
            if let Some(install) = self
                .get_install_cache(&vanilla_id(), &version.clone().into())
                .await?
            {
                install
            } else {
                let install = self.vanilla_install(&version).await?;
                self.set_install_cache(&vanilla_id(), &version.into(), Some(&install))
                    .await?;
                install
            }
        };

        let mc_jar = vanilla_install
            .mc_jar
            .ok_or(anyhow!("missing minecraft jar in vanilla install"))?;
        let mc_jar = self.retrieve_artifact(&mc_jar).await?;

        // prepare variables
        let mut vars = install_profile
            .data
            .into_iter()
            .map(|(k, v)| (k, v.client))
            .chain(once(("SIDE".into(), "client".into())))
            .chain(once(("MINECRAFT_JAR".into(), mc_jar.display().to_string())))
            .collect::<HashMap<_, _>>();

        // special case: BINPATCH /data/client.lzma is packaged in the installer jar
        // extract it first
        let binpatch = container
            .path()
            .join(".installer")
            .join("data")
            .join("client.lzma");
        extract_zip_to(&installer, "data/client.lzma", &binpatch).await?;
        vars.insert("BINPATCH".into(), binpatch.display().to_string());

        container.add_var(vars);
        container.deploy_lib().await?;

        info!("running neoforge install processors");

        for proc in install_profile.processors {
            if !proc
                .sides
                .as_ref()
                .is_none_or(|x| x.contains(&"client".into()))
            {
                debug!("skipping a processor because side mismatch: {proc}");
                continue;
            }

            container.run(&proc).await?;
        }

        info!("collecting neoforge install result");

        let collect = container
            .collect_lib_file(
                java_lib_file
                    .keys()
                    .chain(install.java_lib_class.keys())
                    .chain(install.java_lib_mod.keys())
                    .chain(install.java_lib_file.keys())
                    .chain(vanilla_install.java_lib_class.keys())
                    .chain(vanilla_install.java_lib_mod.keys())
                    .chain(vanilla_install.java_lib_file.keys())
                    .map(|k| k.as_path()),
            )
            .await?;

        container.deinit().await?;

        java_lib_file.extend(collect);

        install.extend(once(Install {
            java_lib_file,
            ..Default::default()
        }));

        install.simplify();

        install.disable_mc_jar = true;

        Ok(install)
    }
}

fn cache_path() -> anyhow::Result<PathBuf> {
    let path = creeper_cache_dir()?.join("builtin").join("neoforge-client");
    Ok(path)
}
