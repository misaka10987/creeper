use reqwest::Client;
use serde::de::DeserializeOwned;
use url::Url;

use crate::{Id, builtin::SyncBuiltinIndex};

const META_API: &str = "https://meta.fabricmc.net/";

pub struct FabricManager {}

impl SyncBuiltinIndex for FabricManager {
    fn package(&self) -> crate::prelude::Id {
        Id::fabric()
    }

    async fn sync_index(&self) -> anyhow::Result<crate::index::Index> {
        todo!()
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
        pub _launcher_meta : Option<serde_json::Value>,
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
