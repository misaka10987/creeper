use clap::Parser;
use url::Url;

use crate::{cmd::Execute, fabric::FabricMod, zip::extract_zip};

#[derive(Clone, Debug, Parser)]
pub struct PackageFabricMod {
    pub url: Url,
}

impl Execute for PackageFabricMod {
    async fn execute(self, lib: &crate::Creeper) -> anyhow::Result<()> {
        let art = lib
            .download(self.url.to_string(), self.url.to_string(), None, None)
            .await?;

        let jar = lib.retrieve_artifact(&art).await?;

        let json = extract_zip(jar, "fabric.mod.json").await?;

        let metadata = serde_json::from_str::<FabricMod>(&json)?;

        println!("{}", metadata.id);

        todo!()
    }
}
