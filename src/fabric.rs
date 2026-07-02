use std::{
    collections::{BTreeSet, HashMap},
    iter::once,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, ensure};
use reqwest::Client;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_with::serde_as;
use tokio::fs::{create_dir_all, read_to_string, try_exists, write};
use tracing::{Span, info, instrument};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use url::Url;

use crate::{
    Creeper, Id, Install,
    builtin::{GetIndex, SyncBuiltinIndex, UpdateIndex},
    index::VersionRev,
    pack::PackNode,
    path::creeper_cache_dir,
    pbar::PROGRESS_STYLE_DEFAULT,
    util::rebuild_req,
    vanilla::check_rule,
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

    pub(crate) async fn fabric_install(&self, version: &Version) -> anyhow::Result<Install> {
        let index = self.get_node(&Id::fabric(), version, 0).await?;

        let req = index
            .dep
            .get(&Id::vanilla())
            .ok_or(anyhow!("fabric@{version} does not have vanilla dependency"))?;

        let index = self.get_index(&Id::vanilla()).await?;

        let all = index.keys().map(|VersionRev(v, _)| v);

        let available = all.filter(|v| req.matches(v)).collect::<BTreeSet<_>>();

        let game = available
            .last()
            .ok_or(anyhow!("no available vanilla version for fabric@{version}"))?;

        let client = FabricMetaClient::new(self.http.clone());

        let profile = client
            .profile(&game.to_string(), &version.to_string())
            .await?;

        let java_flag = profile
            .arguments
            .jvm
            .into_iter()
            .filter_map(|x| x.rules.iter().all(check_rule).then_some(x.values))
            .flatten()
            .collect();

        let mc_flag = profile
            .arguments
            .game
            .into_iter()
            .filter_map(|x| x.rules.iter().all(check_rule).then_some(x.values))
            .flatten()
            .collect();

        let lib = profile
            .libraries
            .into_iter()
            .filter(|x| !(x.name.group == "net.fabricmc" && x.name.artifact == "intermediary"));

        let mut java_lib_class = HashMap::new();

        for lib in lib {
            let path = lib.name.path();

            let art = self
                .download(
                    lib.name.to_string(),
                    lib.url.join(&path.display().to_string())?.to_string(),
                    lib.size,
                    lib.checksum(),
                )
                .await?;

            java_lib_class.insert(path, art);
        }

        let install = Install {
            java_lib_class,
            java_flag,
            java_main_class: Some(profile.main_class),
            mc_flag,
            ..Default::default()
        };

        Ok(install)
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

    pub async fn intermediary_install(&self, version: &Version) -> anyhow::Result<Install> {
        let client = FabricMetaClient::new(self.http.clone());

        let loader = client
            .game_loader_versions(&version.to_string())
            .await?
            .into_iter()
            .filter_map(|v| v.loader.version.parse::<Version>().ok())
            .collect::<BTreeSet<_>>();

        let loader = loader
            .last()
            .ok_or(anyhow!("no fabric loader with intermediary@{version}"))?;

        let profile = client
            .profile(&version.to_string(), &loader.to_string())
            .await?;

        let lib = profile
            .libraries
            .into_iter()
            .filter(|x| x.name.group == "net.fabricmc" && x.name.artifact == "intermediary")
            .collect::<Vec<_>>();

        ensure!(lib.len() == 1, "multiple intermediary libraries found");

        let lib = lib.into_iter().next().unwrap();

        let path = lib.name.path();

        let art = self
            .download(
                lib.name.to_string(),
                lib.url
                    .join(&lib.name.path().display().to_string())?
                    .to_string(),
                lib.size,
                lib.checksum(),
            )
            .await?;

        let install = Install {
            java_lib_class: once((path, art)).collect(),
            ..Default::default()
        };

        Ok(install)
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

    use crate::{Checksum, MavenCoord};

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
}

#[serde_as]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FabricMod {
    pub schema_version: u64,

    pub id: String,

    pub version: Version,

    // #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,

    #[serde(default, skip_serializing_if = "fabric_mod::Contact::is_empty")]
    pub contact: fabric_mod::Contact,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<fabric_mod::Author>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contributors: Vec<fabric_mod::Author>,

    // #[serde_as(as = "Option<DisplayFromStr>")]
    // #[serde(default, skip_serializing_if = "Option::is_none")]
    // pub license: Option<spdx::Expression>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<fabric_mod::Icon>,

    #[serde(default, skip_serializing_if = "is_all")]
    pub environment: fabric_mod::Environment,

    #[serde(default, skip_serializing_if = "fabric_mod::EntryPoints::is_empty")]
    pub entrypoints: fabric_mod::EntryPoints,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jars: Vec<fabric_mod::Jar>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub language_adapters: HashMap<String, String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mixins: Vec<fabric_mod::Mixin>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_widener: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provides: Vec<String>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub depends: HashMap<String, fabric_mod::Dependency>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub recommends: HashMap<String, fabric_mod::Dependency>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub suggests: HashMap<String, fabric_mod::Dependency>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub breaks: HashMap<String, fabric_mod::Dependency>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub conflicts: HashMap<String, fabric_mod::Dependency>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<serde_json::Value>,
}

#[allow(unused)] // used by #[serde(skip_serializing_if = "is_all")]
fn is_all(env: &fabric_mod::Environment) -> bool {
    *env == fabric_mod::Environment::All
}

pub mod fabric_mod {
    use std::{collections::HashMap, path::PathBuf};

    use parse_display::{Display, FromStr};
    use semver::{Version, VersionReq};
    use serde::{Deserialize, Serialize};
    use serde_with::{DeserializeFromStr, SerializeDisplay};
    use tracing::error;
    use url::Url;

    use crate::util::parse_or_prompt;

    #[derive(Clone, Default, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Contact {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub email: Option<String>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub homepage: Option<Url>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub irc: Option<Url>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub issues: Option<Url>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub sources: Option<Url>,

        #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
        pub extra: HashMap<String, Url>,
    }

    impl Contact {
        pub fn is_empty(&self) -> bool {
            self.email.is_none()
                && self.homepage.is_none()
                && self.irc.is_none()
                && self.issues.is_none()
                && self.sources.is_none()
        }
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(untagged, deny_unknown_fields, rename_all = "camelCase")]
    pub enum Author {
        Name(String),
        WithContact { name: String, contact: Contact },
    }

    impl Author {
        pub fn name(self) -> String {
            match self {
                Author::Name(name) => name,
                Author::WithContact { name, .. } => name,
            }
        }
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(untagged, deny_unknown_fields, rename_all = "camelCase")]
    pub enum Icon {
        Single(PathBuf),
        Widths(HashMap<u32, PathBuf>),
    }

    #[derive(
        Clone, Copy, PartialEq, Eq, Display, FromStr, SerializeDisplay, DeserializeFromStr,
    )]
    #[display(style = "camelCase")]
    pub enum Environment {
        #[display("*")]
        All,
        Client,
        Server,
    }

    impl Default for Environment {
        fn default() -> Self {
            Self::All
        }
    }

    #[derive(Clone, Default, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct EntryPoints {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub main: Vec<String>,

        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub client: Vec<String>,

        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub server: Vec<String>,

        #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
        pub extra: HashMap<String, Vec<String>>,
    }

    #[test]
    fn test() {
        let json = r#"{
    "client": [
      "net.caffeinemc.mods.sodium.fabric.SodiumFabricMod"
    ],
    "preLaunch": [
      "net.caffeinemc.mods.sodium.fabric.SodiumPreLaunch"
    ]
  }"#;
        serde_json::from_str::<EntryPoints>(json).unwrap();
    }

    impl EntryPoints {
        pub fn is_empty(&self) -> bool {
            self.main.is_empty() && self.client.is_empty() && self.server.is_empty()
        }
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    pub struct Jar {
        pub file: PathBuf,
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(untagged, deny_unknown_fields, rename_all = "camelCase")]
    pub enum Mixin {
        Config(PathBuf),
        WithEnvironment {
            config: PathBuf,
            environment: Environment,
        },
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(untagged, deny_unknown_fields, rename_all = "camelCase")]
    pub enum Dependency {
        Req(VersionReq),

        List(Vec<Version>),

        VersionReq(String),

        VersionList(Vec<String>),
    }

    impl Dependency {
        pub async fn prompt_normalize(&self) -> anyhow::Result<VersionReq> {
            let req = match self {
                crate::fabric::fabric_mod::Dependency::Req(req) => req.clone(),
                crate::fabric::fabric_mod::Dependency::List(_) => {
                    error!(
                        "does not support list of versions in fabric dependency, defaulting to *"
                    );
                    VersionReq::STAR
                }
                crate::fabric::fabric_mod::Dependency::VersionReq(req) => {
                    parse_or_prompt(&req, "version requirement").await?
                }
                crate::fabric::fabric_mod::Dependency::VersionList(_) => {
                    error!(
                        "does not support list of versions in fabric dependency, defaulting to *"
                    );
                    VersionReq::STAR
                }
            };

            Ok(req)
        }
    }
}
