use clap::Parser;
use url::Url;

use crate::cmd::Execute;

/// Download file from URL and generate artifact information.
///
/// This is useful for packagers.
#[derive(Clone, Debug, Parser)]
pub struct Download {
    /// File to download.
    #[arg(value_name = "URL")]
    pub url: Url,

    /// Name of the artifact.
    /// Will use the URL if not specified.
    #[arg(long)]
    pub name: Option<String>,
}

impl Execute for Download {
    async fn execute(self, lib: &crate::Creeper) -> anyhow::Result<()> {
        let name = self.name.unwrap_or(self.url.to_string());

        let art = lib.download(name, self.url.to_string(), None, None).await?;

        let toml = toml::to_string_pretty(&art)?;

        println!("{toml}");

        Ok(())
    }
}
