pub mod meta;
pub mod meta_api;
mod prelude;

pub use prelude::*;

use std::{
    collections::{BTreeSet, HashMap},
    iter::once,
    time::Duration,
};

use anyhow::{anyhow, ensure};
use reqwest::Client;
use semver::{Version, VersionReq};
use tracing::{Span, info, instrument};
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::{
    Creeper, Id, Install,
    builtin::{SyncBuiltinIndex, UpdateIndex},
    fabric::meta_api::{Game, LoaderWithIntermediary},
    index::VersionRev,
    pack::PackNode,
    pbar::PROGRESS_STYLE_DEFAULT,
    util::rebuild_req,
    vanilla::RuleChecker,
};

pub struct FabricManager {
    http: Client,
}

impl FabricManager {
    pub fn new(http: Client) -> Self {
        Self { http }
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
            .filter_map(|Game { version, stable }| stable.then_some(version))
            .filter_map(|v| v.parse::<Version>().ok())
            .collect::<Vec<_>>();

        let mut map = HashMap::<Version, Vec<Version>>::new();

        let span = Span::current();
        span.pb_set_message("versions");
        span.pb_set_style(&PROGRESS_STYLE_DEFAULT);
        span.pb_set_length(games.len() as u64);

        for v in &games {
            let loaders = client.game_loader_versions(&v.to_string()).await?;

            let loaders =
                loaders
                    .into_iter()
                    .filter_map(|LoaderWithIntermediary { loader, .. }| {
                        loader.version.parse::<Version>().ok()
                    });

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
                    VersionRev::new(k),
                    PackNode {
                        dep: [(Id::vanilla(), v), (Id::intermediary(), VersionReq::STAR)]
                            .into_iter()
                            .collect(),
                        conflict: once((Id::neoforge(), VersionReq::STAR)).collect(),
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
    #[instrument(skip(self))]
    pub async fn update_fabric(&self) -> anyhow::Result<()> {
        if self.args.offline {
            info!("skipping fabric update because offline mode enabled");
            return Ok(());
        }

        self.fabric.update_index().await?;

        Ok(())
    }

    pub(crate) async fn fabric_install(&self, version: &Version) -> anyhow::Result<Install> {
        let index = self.get_node(&Id::fabric(), version, 0).await?;

        let req = index
            .dep
            .get(&Id::vanilla())
            .ok_or(anyhow!("fabric@{version} does not have vanilla dependency"))?;

        let index = self.get_index(&Id::vanilla()).await?;

        let all = index.keys().map(|VersionRev { version, .. }| version);

        let available = all.filter(|v| req.matches(v)).collect::<BTreeSet<_>>();

        let game = available
            .last()
            .ok_or(anyhow!("no available vanilla version for fabric@{version}"))?;

        let client = FabricMetaClient::new(self.http.clone());

        let profile = client
            .profile(&game.to_string(), &version.to_string())
            .await?;

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
            java_lib_class.insert(path, (lib.name.to_string(), src, lib.size, lib.checksum()));
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
                    VersionRev::new(v.clone()),
                    PackNode {
                        dep: once((Id::vanilla(), format!("={v}").parse().unwrap())).collect(),
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
    #[instrument(skip(self))]
    pub async fn update_intermediary(&self) -> anyhow::Result<()> {
        if self.args.offline {
            info!("skipping intermediary update because offline mode enabled");
            return Ok(());
        }

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
