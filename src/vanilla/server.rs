use std::time::Duration;

use anyhow::anyhow;
use semver::Version;

use crate::{
    Checksum, Creeper, Id, Install, VersionRev, builtin::SyncBuiltinIndex,
    index::independent_index, jar::jar_main_class, mc::ManifestClient,
};

pub struct VanillaServerManager {
    manifest: ManifestClient,
}

impl VanillaServerManager {
    pub fn new(manifest: ManifestClient) -> Self {
        Self { manifest }
    }
}

impl SyncBuiltinIndex for VanillaServerManager {
    fn package(&self) -> Id {
        Id::vanilla_server()
    }

    async fn sync_index(&self) -> anyhow::Result<crate::index::Index> {
        let versions = self.manifest.get_version_list().await?;

        let index = independent_index(versions.into_iter().cloned().map(VersionRev::new));

        Ok(index)
    }

    fn cache_expiry(&self) -> std::time::Duration {
        Duration::from_hours(72)
    }
}

impl Creeper {
    pub(crate) async fn vanilla_server_install(
        &self,
        version: &Version,
    ) -> anyhow::Result<Install> {
        let mc_version = self.vanilla_server.manifest.get_version(version).await?;

        let server = mc_version
            .downloads
            .server
            .ok_or(anyhow!("missing server in vanilla manifest {version}"))?;

        let server = self
            .download(
                format!("minecraft_server.{}.jar", mc_version.id),
                server.url,
                Some(server.size),
                [Checksum::sha1(server.sha1)],
            )
            .await?;

        let jar = self.retrieve_artifact(&server).await?;

        let main_class = jar_main_class(jar).await?;

        let install = Install {
            mc_jar: Some(server),

            java_main_class: Some(main_class),

            mc_flag: vec!["nogui".into()],

            ..Default::default()
        };

        Ok(install)
    }
}
