use std::{
    collections::HashMap,
    iter::once,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::Client;
use semver::{Version, VersionReq};
use serde::de::DeserializeOwned;
use tokio::fs::{create_dir_all, read_to_string, try_exists, write};
use tracing::{Span, info, instrument};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use url::Url;

use crate::{
    Creeper, Id,
    builtin::{GetIndex, SyncBuiltinIndex, UpdateIndex},
    index::VersionRev,
    pack::PackNode,
    path::creeper_cache_dir,
    pbar::PROGRESS_STYLE_DEFAULT,
    util::rebuild_req,
};

const META_API: &str = "https://meta.fabricmc.net/";

pub struct FabricManager {
    http: Client,
}

impl FabricManager {
    pub fn new(http: Client) -> Self {
        Self { http }
    }

    fn cache_path() -> anyhow::Result<PathBuf> {
        let path = creeper_cache_dir()?.join("builtin").join("fabric");
        Ok(path)
    }

    async fn since_last_index_update(&self) -> anyhow::Result<Option<Duration>> {
        let path = Self::cache_path()?.join("index-last-updated");

        if !try_exists(&path).await? {
            return Ok(None);
        }

        let time = read_to_string(path).await?.parse::<u64>()?;

        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(time);

        let duration = time.elapsed().ok();

        Ok(duration)
    }

    async fn renew_index_last_update(&self) -> anyhow::Result<()> {
        let path = Self::cache_path()?.join("index-last-updated");

        create_dir_all(path.parent().unwrap()).await?;

        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        write(path, time.to_string()).await?;

        Ok(())
    }
}

impl SyncBuiltinIndex for FabricManager {
    fn package(&self) -> crate::prelude::Id {
        Id::fabric()
    }

    async fn sync_index(&self) -> anyhow::Result<crate::index::Index> {
        let client = FabricMetaClient::new(self.http.clone());

        let games = client.game_versions().await?;

        let games = games
            .into_iter()
            .filter_map(|fabric_meta::Game { version, stable }| stable.then_some(version))
            .filter_map(|v| v.parse::<Version>().ok())
            .collect::<Vec<_>>();

        let mut map = HashMap::<Version, Vec<Version>>::new();

        let span = Span::current();
        span.pb_set_message("versions");
        span.pb_set_style(&PROGRESS_STYLE_DEFAULT);
        span.pb_set_length(games.len() as u64);

        for v in &games {
            let loaders = client.game_loader_versions(&v.to_string()).await?;

            let loaders = loaders.into_iter().filter_map(
                |fabric_meta::LoaderWithIntermediary { loader, .. }| {
                    loader.version.parse::<Version>().ok()
                },
            );

            for loader in loaders {
                map.entry(loader).or_default().push(v.clone());
            }

            span.pb_inc(1);
        }

        let index = map
            .into_iter()
            .filter_map(|(k, v)| {
                rebuild_req(v.into_iter().collect(), games.clone().into_iter().collect())
                    .ok()
                    .map(|v| (k, v))
            })
            .map(|(k, v)| {
                (
                    VersionRev(k, 0),
                    PackNode {
                        dep: [(Id::vanilla(), v), (Id::intermediary(), VersionReq::STAR)]
                            .into_iter()
                            .collect(),
                        conflict: vec![once((Id::neoforge(), VersionReq::STAR)).collect()],
                        ..Default::default()
                    },
                )
            })
            .collect();

        self.renew_index_last_update().await?;

        Ok(index)
    }
}

impl Creeper {
    #[instrument(skip(self))]
    pub async fn update_fabric(&self) -> anyhow::Result<()> {
        if self.fabric.get_index().await.is_ok()
            && let Some(time) = self.fabric.since_last_index_update().await?
            && time < Duration::from_secs(60 * 60 * 24 * 14)
        {
            info!("skipping slow fabric index update since already updated in 14 days");
        } else {
            self.fabric.update_index().await?;
        }

        Ok(())
    }
}

pub struct IntermediaryManager {
    http: Client,
}

impl IntermediaryManager {
    pub fn new(http: Client) -> Self {
        Self { http }
    }
}

impl SyncBuiltinIndex for IntermediaryManager {
    fn package(&self) -> Id {
        Id::intermediary()
    }

    async fn sync_index(&self) -> anyhow::Result<crate::index::Index> {
        let client = FabricMetaClient::new(self.http.clone());

        let versions = client.intermediary_versions().await?;

        let versions = versions
            .into_iter()
            .filter_map(|v| v.version.parse::<Version>().ok());

        let index = versions
            .map(|v| {
                (
                    VersionRev(v.clone(), 0),
                    PackNode {
                        dep: once((Id::vanilla(), format!("={v}").parse().unwrap())).collect(),
                        ..Default::default()
                    },
                )
            })
            .collect();

        Ok(index)
    }
}

impl Creeper {
    #[instrument(skip(self))]
    pub async fn update_intermediary(&self) -> anyhow::Result<()> {
        self.intermediary.update_index().await
    }
}

pub struct FabricMetaClient {
    http: Client,
}

impl FabricMetaClient {
    pub fn new(http: Client) -> Self {
        Self { http }
    }

    async fn get_meta<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let path = path.strip_prefix("/").unwrap_or(path);

        let url = META_API.parse::<Url>().unwrap().join(path)?;

        let res = self.http.get(url).send().await?.json().await?;

        Ok(res)
    }

    pub async fn game_versions(&self) -> anyhow::Result<Vec<fabric_meta::Game>> {
        self.get_meta("/v2/versions/game").await
    }

    pub async fn game_versions_yarn(&self) -> anyhow::Result<Vec<fabric_meta::Game>> {
        self.get_meta("/v2/versions/game/yarn").await
    }

    pub async fn game_versions_intermediary(&self) -> anyhow::Result<Vec<fabric_meta::Game>> {
        self.get_meta("/v2/versions/game/intermediary").await
    }

    pub async fn intermediary_versions(&self) -> anyhow::Result<Vec<fabric_meta::Intermediary>> {
        self.get_meta("/v2/versions/intermediary").await
    }

    pub async fn game_intermediary_versions(
        &self,
        game: &str,
    ) -> anyhow::Result<Vec<fabric_meta::Intermediary>> {
        let path = format!("/v2/versions/intermediary/{game}");

        self.get_meta(&path).await
    }

    pub async fn yarn_versions(&self) -> anyhow::Result<Vec<fabric_meta::Mapping>> {
        self.get_meta("/v2/versions/yarn").await
    }

    pub async fn game_yarn_versions(
        &self,
        game: &str,
    ) -> anyhow::Result<Vec<fabric_meta::Mapping>> {
        let path = format!("/v2/versions/yarn/{game}");

        self.get_meta(&path).await
    }

    pub async fn loader_versions(&self) -> anyhow::Result<Vec<fabric_meta::Loader>> {
        self.get_meta("/v2/versions/loader").await
    }

    pub async fn game_loader_versions(
        &self,
        game: &str,
    ) -> anyhow::Result<Vec<fabric_meta::LoaderWithIntermediary>> {
        let path = format!("/v2/versions/loader/{game}");

        self.get_meta(&path).await
    }

    pub async fn profile(&self, game: &str, loader: &str) -> anyhow::Result<fabric_meta::Profile> {
        let path = format!("/v2/versions/loader/{game}/{loader}/profile/json");

        self.get_meta(&path).await
    }
}

pub mod fabric_meta {
    use mc_launchermeta::{VersionKind, version::Arguments};
    use serde::{Deserialize, Serialize};
    use url::Url;

    use crate::MavenCoord;

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    pub struct Game {
        /// The version of the game.
        ///
        /// Minecraft's version number may not be a valid semver.
        pub version: String,

        pub stable: bool,
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    pub struct Mapping {
        pub game_version: String,
        pub separator: String,
        pub build: u64,
        pub maven: MavenCoord,
        pub version: String,
        pub stable: bool,
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    pub struct Intermediary {
        pub maven: MavenCoord,
        pub version: String,
        pub stable: bool,
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    pub struct Loader {
        pub separator: String,
        pub build: u64,
        pub maven: MavenCoord,
        pub version: String,
        pub stable: bool,
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    pub struct Installer {
        pub url: Url,
        pub maven: MavenCoord,
        pub version: String,
        pub stable: bool,
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    pub struct LoaderWithIntermediary {
        pub loader: Loader,
        pub intermediary: Intermediary,
        #[serde(rename = "launcherMeta")]
        pub _launcher_meta: Option<serde_json::Value>,
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    pub struct Profile {
        pub id: String,

        pub inherits_from: String,

        pub release_time: String,

        pub time: String,

        #[serde(rename = "type")]
        pub kind: VersionKind,

        pub main_class: String,

        pub arguments: Arguments,

        pub libraries: Vec<Library>,
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    pub struct Library {
        pub name: MavenCoord,
        pub url: Url,
        pub md5: String,
        pub sha1: String,
        pub sha256: String,
        pub sha512: String,
        pub size: u64,
    }
}
