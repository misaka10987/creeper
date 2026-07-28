mod client;
mod manifest;
mod prelude;
mod server;

use std::time::Duration;

pub use prelude::*;

use crate::{
    VersionRev,
    builtin::{SyncBuiltinIndex, client_id, minecraft_id, server_id},
    pack::PackNode,
};

pub struct MinecraftManager {
    manifest: ManifestClient,
}

impl MinecraftManager {
    pub fn new(manifest: ManifestClient) -> Self {
        Self { manifest }
    }
}

impl SyncBuiltinIndex for MinecraftManager {
    fn package(&self) -> crate::prelude::Id {
        minecraft_id()
    }

    async fn sync_index(&self) -> anyhow::Result<crate::index::Index> {
        let versions = self.manifest.get_version_list().await?;

        let index = versions
            .into_iter()
            .map(|v| {
                let grp = [
                    (client_id(), format!("={v}").parse().unwrap()),
                    (server_id(), format!("={v}").parse().unwrap()),
                    (
                        "minecraft-provider".parse().unwrap(),
                        format!("={v}").parse().unwrap(),
                    ),
                ];

                let either_dep = vec![grp.into()];

                let node = PackNode {
                    either_dep,
                    ..Default::default()
                };

                (VersionRev::new(v.clone()), node)
            })
            .collect();

        Ok(index)
    }

    fn cache_expiry(&self) -> std::time::Duration {
        Duration::from_hours(72)
    }
}
