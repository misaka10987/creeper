use mc_launchermeta::{VersionKind, version::Arguments};
use reqwest::Client;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use url::Url;

use crate::{Checksum, MavenCoord};

pub const META_API: &str = "https://meta.fabricmc.net/";

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

    pub async fn versions(&self) -> anyhow::Result<FabricMeta> {
        self.get_meta("/v2/versions").await
    }

    pub async fn game_versions(&self) -> anyhow::Result<Vec<Game>> {
        self.get_meta("/v2/versions/game").await
    }

    pub async fn game_versions_yarn(&self) -> anyhow::Result<Vec<Game>> {
        self.get_meta("/v2/versions/game/yarn").await
    }

    pub async fn game_versions_intermediary(&self) -> anyhow::Result<Vec<Game>> {
        self.get_meta("/v2/versions/game/intermediary").await
    }

    pub async fn intermediary_versions(&self) -> anyhow::Result<Vec<Intermediary>> {
        self.get_meta("/v2/versions/intermediary").await
    }

    pub async fn game_intermediary_versions(
        &self,
        game: &str,
    ) -> anyhow::Result<Vec<Intermediary>> {
        let path = format!("/v2/versions/intermediary/{game}");

        self.get_meta(&path).await
    }

    pub async fn yarn_versions(&self) -> anyhow::Result<Vec<Mapping>> {
        self.get_meta("/v2/versions/yarn").await
    }

    pub async fn game_yarn_versions(&self, game: &str) -> anyhow::Result<Vec<Mapping>> {
        let path = format!("/v2/versions/yarn/{game}");

        self.get_meta(&path).await
    }

    pub async fn loader_versions(&self) -> anyhow::Result<Vec<Loader>> {
        self.get_meta("/v2/versions/loader").await
    }

    pub async fn game_loader_versions(
        &self,
        game: &str,
    ) -> anyhow::Result<Vec<LoaderWithIntermediary>> {
        let path = format!("/v2/versions/loader/{game}");

        self.get_meta(&path).await
    }

    pub async fn profile(&self, game: &str, loader: &str) -> anyhow::Result<Profile> {
        let path = format!("/v2/versions/loader/{game}/{loader}/profile/json");

        self.get_meta(&path).await
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FabricMeta {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub game: Vec<Game>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mappings: Vec<Mapping>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intermediary: Vec<Intermediary>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loader: Vec<Loader>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub installer: Vec<Installer>,
}

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
    pub md5: Option<String>,
    pub sha1: Option<String>,
    pub sha256: Option<String>,
    pub sha512: Option<String>,
    pub size: Option<u64>,
}

impl Library {
    pub fn checksum(self) -> impl IntoIterator<Item = Checksum> {
        self.sha1
            .into_iter()
            .map(Checksum::sha1)
            .chain(self.sha256.into_iter().map(Checksum::sha256))
    }
}
