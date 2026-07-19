use std::time::Duration;

use anyhow::anyhow;
use mc_launchermeta::{VERSION_MANIFEST_URL, version_manifest::Manifest};
use reqwest::Client;
use semver::Version;
use tracing::{debug, info, trace};

use crate::{
    Checksum, Creeper, Id, Install, VersionRev,
    builtin::{SyncBuiltinIndex, UpdateIndex},
    index::independent_index,
    util::JarManifest,
    zip::extract_zip,
};

pub struct ServerManager {
    http: Client,
}

impl ServerManager {
    pub fn new(http: Client) -> Self {
        Self { http }
    }
}

impl SyncBuiltinIndex for ServerManager {
    fn package(&self) -> Id {
        Id::server()
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
    pub async fn update_server(&self) -> anyhow::Result<()> {
        if self.args.offline {
            info!("skipping vanilla update because offline mode enabled");
            return Ok(());
        }

        self.server.update_index().await
    }

    pub(crate) async fn server_install(&self, version: &Version) -> anyhow::Result<Install> {
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

        let manifest = extract_zip(jar, "META-INF/MANIFEST.MF")
            .await?
            .parse::<JarManifest>()?;

        let main_class = manifest
            .main_class
            .ok_or(anyhow!("{} missing main class", server.name))?;

        let install = Install {
            java_lib_class: [(server.name.clone().into(), server)].into_iter().collect(),

            java_main_class: Some(main_class),

            mc_flag: vec!["nogui".into()],

            ..Default::default()
        };

        Ok(install)
    }
}
