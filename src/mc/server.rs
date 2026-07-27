use std::time::Duration;

use crate::{
    Id, VersionRev, builtin::SyncBuiltinIndex, index::Index, mc::manifest::ManifestClient,
    neoforge::mc_nf_req, pack::PackNode,
};

pub struct ServerManager {
    manifest: ManifestClient,
}

impl ServerManager {
    pub fn new(manifest: ManifestClient) -> Self {
        Self { manifest }
    }
}

impl SyncBuiltinIndex for ServerManager {
    fn package(&self) -> Id {
        Id::server()
    }

    async fn sync_index(&self) -> anyhow::Result<Index> {
        let versions = self.manifest.get_version_list().await?;

        let index = versions
            .into_iter()
            .map(|v| {
                let grp = [
                    (Id::vanilla_server(), format!("={v}").parse().unwrap()),
                    (Id::neoforge_server(), mc_nf_req(v)),
                    (
                        "server-provider".parse().unwrap(),
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
