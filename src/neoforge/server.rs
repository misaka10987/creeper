use std::time::Duration;

use reqwest::Client;
use semver::Version;
use tracing::{debug, info};

use crate::{
    Creeper, Id, Install, VersionRev,
    builtin::{SyncBuiltinIndex, UpdateIndex},
    index::{Index, independent_index},
    neoforge::{parse_neoforge_version, query_neoforge_versions},
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
        todo!()
    }
}
