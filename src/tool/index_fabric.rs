use std::{collections::HashMap, iter::once, path::PathBuf};

use clap::Parser;
use colored::Colorize;
use semver::{Version, VersionReq};
use tracing::{Span, instrument};
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::{
    FabricMetaClient, Id,
    cmd::Execute,
    fabric::fabric_meta,
    index::{Index, IndexLine, VersionRev},
    pack::PackNode,
    pbar::PROGRESS_STYLE_DEFAULT,
    util::rebuild_req,
};

#[derive(Clone, Debug, Parser)]
pub struct IndexFabric {
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

impl Execute for IndexFabric {
    #[instrument(name = "index-fabric", skip(self, lib))]
    async fn execute(self, lib: &crate::Creeper) -> anyhow::Result<()> {
        let client = FabricMetaClient::new(lib.http.clone());

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
                        dep: once((Id::vanilla(), v)).collect(),
                        conflict: once((Id::neoforge(), VersionReq::STAR)).collect(),
                        ..Default::default()
                    },
                )
            })
            .collect::<Index>();

        let len = index.len();

        let jsonl = IndexLine::to_jsonl(&Id::fabric(), index.clone())?;

        eprintln!("{} {} Fabric versions", "Indexed".bold().green(), len);

        println!("{jsonl}");

        if let Some(output) = self.output {
            IndexLine::write(output, &Id::fabric(), index).await?;
        }

        Ok(())
    }
}
