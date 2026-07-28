use std::time::Duration;

use crate::{
    VersionRev,
    builtin::{SyncBuiltinIndex, client_id, vanilla_id},
    mc::ManifestClient,
    pack::PackNode,
};

pub struct ClientManager {
    manifest: ManifestClient,
}

impl ClientManager {
    pub fn new(manifest: ManifestClient) -> Self {
        Self { manifest }
    }
}

impl SyncBuiltinIndex for ClientManager {
    fn package(&self) -> crate::prelude::Id {
        client_id()
    }

    async fn sync_index(&self) -> anyhow::Result<crate::index::Index> {
        let versions = self.manifest.get_version_list().await?;

        let index = versions
            .into_iter()
            .map(|v| {
                let grp = [
                    (vanilla_id(), format!("={v}").parse().unwrap()),
                    (
                        "client-provider".parse().unwrap(),
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
