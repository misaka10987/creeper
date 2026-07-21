use std::time::Duration;

use anyhow::anyhow;
use mc_launchermeta::{VERSION_MANIFEST_URL, version_manifest::Manifest};
use reqwest::Client;
use semver::Version;
use tracing::{debug, trace};

use crate::{
    Checksum, Creeper, Id, Install, VersionRev, builtin::SyncBuiltinIndex,
    index::independent_index, jar::jar_main_class,
};

pub struct VanillaServerManager {
    http: Client,
}

impl VanillaServerManager {
    pub fn new(http: Client) -> Self {
        Self { http }
    }
}

impl SyncBuiltinIndex for VanillaServerManager {
    fn package(&self) -> Id {
        Id::vanilla_server()
    }

    async fn sync_index(&self) -> anyhow::Result<crate::index::Index> {
        let manifest = self
            .http
            .get(VERSION_MANIFEST_URL)
            .send()
            .await?
            .error_for_status()?
            .json::<Manifest>()
            .await?;

        let mut versions = vec![];

        let count = manifest.versions.len();

        for version in manifest.versions {
            if let Some(version) = version.id.parse().ok() {
                versions.push(version);
            } else {
                trace!("ignoring invalid vanilla version {}", version.id);
            }
        }

        debug!(
            "retrieved {count} vanilla versions, of which {} valid",
            versions.len()
        );

        let index = independent_index(versions.into_iter().map(VersionRev::new));

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
        let mc_version = self.vanilla_version(version.clone()).await?;

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
