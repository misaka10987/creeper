use clap::Parser;
use url::Url;

use crate::{cmd::Execute, neoforge::NeoforgeMods, zip::extract_zip};

/// Package a NeoForge mod by downloading form the specified URL and parsing JAR metadata.
#[derive(Clone, Debug, Parser)]
pub struct PackageNeoforgeMod {
    /// Download URL for the NeoForge mod JAR file.
    ///
    /// Note that this URL will be used in the resulting package as download source.
    pub url: Url,
}

impl Execute for PackageNeoforgeMod {
    async fn execute(self, lib: &crate::Creeper) -> anyhow::Result<()> {
        let art = lib
            .download(self.url.to_string(), self.url.to_string(), None, None)
            .await?;

        let jar = lib.retrieve_artifact(&art).await?;

        let toml = extract_zip(jar, "META-INF/neoforge.mods.toml").await?;

        let mods = toml::from_str::<NeoforgeMods>(&toml)?;

        todo!()
    }
}
