use std::{collections::HashMap, iter::once, path::PathBuf};

use anyhow::{anyhow, ensure};
use mc_launchermeta::version as mc_version;
use serde::{Deserialize, Serialize};
use tokio::fs::read_to_string;

use crate::{Artifact, Checksum, Creeper, Install};

#[derive(Clone, Serialize, Deserialize)]
pub struct AssetIndex {
    pub objects: HashMap<PathBuf, Object>,
}

impl AssetIndex {
    pub fn from_map(map: HashMap<PathBuf, Artifact>) -> anyhow::Result<Self> {
        let mut objects = HashMap::new();

        for (path, art) in map {
            let sha1 = art
                .sha1
                .ok_or(anyhow!("missing SHA-1 checksum in asset index"))?;
            objects.insert(
                path,
                Object {
                    sha1,
                    size: art.len,
                },
            );
        }

        Ok(Self { objects })
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Object {
    #[serde(rename = "hash")]
    pub sha1: String,
    pub size: u64,
}

impl Creeper {
    pub(crate) async fn download_asset_index(
        &self,
        download: mc_version::AssetIndex,
    ) -> anyhow::Result<AssetIndex> {
        let name = format!("assets/indexes/{}.json", download.id);

        let art = self
            .download(
                name,
                download.url,
                Some(download.size),
                Some(Checksum::sha1(download.sha1)),
            )
            .await?;

        let file = self.retrieve_artifact(&art).await?;

        let json = read_to_string(file).await?;

        let index = serde_json::from_str::<AssetIndex>(&json)?;

        Ok(index)
    }

    pub async fn vanilla_asset_install(&self, index: AssetIndex) -> anyhow::Result<Install> {
        let mut map = HashMap::new();

        for (path, obj) in index.objects {
            let name = path.display().to_string();

            let src = asset_download_url(&obj.sha1)?;

            map.insert(
                path,
                (name, src, Some(obj.size), once(Checksum::sha1(obj.sha1))),
            );
        }

        let map = self.batch_download(map).await?;

        let value = Install {
            mc_asset: map,
            ..Default::default()
        };

        Ok(value)
    }
}

pub fn asset_download_url(sha1: &str) -> anyhow::Result<String> {
    ensure!(sha1.len() == 40, "invalid sha1 length");
    let first2 = &sha1[0..2];
    let url = format!("https://resources.download.minecraft.net/{first2}/{sha1}");
    Ok(url)
}
