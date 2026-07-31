pub mod meta;
mod prelude;

use futures::{StreamExt, TryStreamExt, stream};
pub use prelude::*;

use std::{
    collections::{BTreeSet, HashMap},
    iter::once,
    time::Duration,
};

use anyhow::{anyhow, ensure};
use fabric_meta_api::{FabricMetaClient, Game, Library, LoaderWithIntermediary};
use semver::{Version, VersionReq};
use tracing::{Span, instrument};
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::{
    Checksum, Creeper, Id, Install,
    builtin::{SyncBuiltinIndex, fabric_id, intermediary_id, neoforge_id, vanilla_id},
    http::HttpThrottle,
    index::VersionRev,
    pack::PackNode,
    pbar::PROGRESS_STYLE_DEFAULT,
    util::rebuild_req,
    vanilla::RuleChecker,
};

pub struct FabricManager {
    pub parallel_download: usize,
    http: HttpThrottle,
}

impl FabricManager {
    pub fn new(http: HttpThrottle, parallel_download: usize) -> Self {
        Self {
            http,
            parallel_download,
        }
    }
}

impl SyncBuiltinIndex for FabricManager {
    fn package(&self) -> crate::prelude::Id {
        fabric_id()
    }

    #[instrument(skip(self))]
    async fn sync_index(&self) -> anyhow::Result<crate::index::Index> {
        let req = self.http.req().await;

        let client = FabricMetaClient::new(req.as_client());

        let games = client.game_versions().await?;

        drop(req);

        let games = games
            .into_iter()
            .filter_map(|Game { version, stable }| stable.then_some(version))
            .filter_map(|v| v.parse::<Version>().ok())
            .collect::<Vec<_>>();

        let span = Span::current();

        // span.pb_set_message(span.metadata().unwrap().name());
        span.pb_set_style(&PROGRESS_STYLE_DEFAULT);
        span.pb_set_length(games.len() as u64);

        // game version to supported loader versions
        let game_loader = stream::iter(games.clone())
            .map(|v| async move {
                let req = self.http.req().await;

                let client = FabricMetaClient::new(self.http.req().await.as_client());

                let loaders = client.game_loader_versions(&v.to_string()).await;

                drop(req);

                Span::current().pb_inc(1);

                loaders.map(|loaders| (v.clone(), loaders))
            })
            .buffer_unordered(self.parallel_download)
            .try_collect::<HashMap<_, _>>()
            .await?;

        let mut loader_game = HashMap::<Version, Vec<Version>>::new();

        for (game, loaders) in game_loader {
            let loaders = loaders
                .into_iter()
                .filter_map(|LoaderWithIntermediary { loader, .. }| loader.version.parse().ok());

            for loader in loaders {
                loader_game.entry(loader).or_default().push(game.clone());
            }
        }

        let index = loader_game
            .into_iter()
            .filter_map(|(k, v)| {
                rebuild_req(v.into_iter().collect(), games.clone().into_iter().collect())
                    .ok()
                    .map(|v| (k, v))
            })
            .map(|(k, v)| {
                (
                    VersionRev::new(k),
                    PackNode {
                        dep: [(vanilla_id(), v), (intermediary_id(), VersionReq::STAR)]
                            .into_iter()
                            .collect(),
                        conflict: once((neoforge_id(), VersionReq::STAR)).collect(),
                        ..Default::default()
                    },
                )
            })
            .collect();

        Ok(index)
    }

    fn cache_expiry(&self) -> std::time::Duration {
        // 14 days
        Duration::from_hours(14 * 24)
    }
}

impl Creeper {
    pub(crate) async fn fabric_install(&self, version: &Version) -> anyhow::Result<Install> {
        let index = self.get_node(&fabric_id(), version, 0).await?;

        let req = index
            .dep
            .get(&vanilla_id())
            .ok_or(anyhow!("fabric@{version} does not have vanilla dependency"))?;

        let index = self.get_index(&vanilla_id()).await?;

        let all = index.keys().map(|VersionRev { version, .. }| version);

        let available = all.filter(|v| req.matches(v)).collect::<BTreeSet<_>>();

        let game = available
            .last()
            .ok_or(anyhow!("no available vanilla version for fabric@{version}"))?;

        let req = self.http.req().await;

        let client = FabricMetaClient::new(req.as_client());

        let profile = client
            .profile(&game.to_string(), &version.to_string())
            .await?;

        drop(req);

        let rule = RuleChecker::default();

        let java_flag = profile
            .arguments
            .jvm
            .into_iter()
            .filter_map(|x| x.rules.iter().all(rule.checker()).then_some(x.values))
            .flatten()
            .collect();

        let mc_flag = profile
            .arguments
            .game
            .into_iter()
            .filter_map(|x| x.rules.iter().all(rule.checker()).then_some(x.values))
            .flatten()
            .collect();

        let lib = profile
            .libraries
            .into_iter()
            .filter(|x| !(x.name.group == "net.fabricmc" && x.name.artifact == "intermediary"));

        let mut java_lib_class = HashMap::new();

        for lib in lib {
            let path = lib.name.path();
            let src = lib.url.join(&path.display().to_string())?.to_string();
            java_lib_class.insert(path, (lib.name.to_string(), src, lib.size, checksum(lib)));
        }

        let java_lib_class = self.batch_download(java_lib_class).await?;

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
    http: HttpThrottle,
}

impl IntermediaryManager {
    pub fn new(http: HttpThrottle) -> Self {
        Self { http }
    }
}

impl SyncBuiltinIndex for IntermediaryManager {
    fn package(&self) -> Id {
        intermediary_id()
    }

    async fn sync_index(&self) -> anyhow::Result<crate::index::Index> {
        let req = self.http.req().await;

        let client = FabricMetaClient::new(req.as_client());

        let versions = client.intermediary_versions().await?;

        drop(req);

        let versions = versions
            .into_iter()
            .filter_map(|v| v.version.parse::<Version>().ok());

        let index = versions
            .map(|v| {
                (
                    VersionRev::new(v.clone()),
                    PackNode {
                        dep: once((vanilla_id(), format!("={v}").parse().unwrap())).collect(),
                        ..Default::default()
                    },
                )
            })
            .collect();

        Ok(index)
    }

    fn cache_expiry(&self) -> Duration {
        Duration::from_hours(72)
    }
}

impl Creeper {
    pub(crate) async fn intermediary_install(&self, version: &Version) -> anyhow::Result<Install> {
        let req = self.http.req().await;

        let client = FabricMetaClient::new(req.as_client());

        let loader = client
            .game_loader_versions(&version.to_string())
            .await?
            .into_iter()
            .filter_map(|v| v.loader.version.parse::<Version>().ok())
            .collect::<BTreeSet<_>>();

        drop(req);

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
                checksum(lib),
            )
            .await?;

        let install = Install {
            java_lib_class: once((path, art)).collect(),
            ..Default::default()
        };

        Ok(install)
    }
}

fn checksum(lib: Library) -> impl IntoIterator<Item = Checksum> {
    lib.sha1
        .into_iter()
        .map(Checksum::sha1)
        .chain(lib.sha256.into_iter().map(Checksum::sha256))
}
