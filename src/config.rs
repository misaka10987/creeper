use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use tokio::fs::{read_to_string, write};
use tracing::info;
use url::Url;

use crate::Creeper;

#[serde_inline_default]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    /// URL to the package registry.
    #[serde_inline_default("https://creeper-registry.pages.dev/".parse().unwrap())]
    #[serde(skip_serializing_if = "is_default_registry")]
    pub registry: Url,

    /// Maximum number of parallel jobs.
    #[serde_inline_default(4)]
    #[serde(skip_serializing_if = "is_4")]
    pub parallel_job: usize,

    /// Maximum number of parallel HTTP requests.
    #[serde_inline_default(8)]
    #[serde(skip_serializing_if = "is_8")]
    pub parallel_http: usize,

    #[serde_inline_default(false)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub use_bmclapi: bool,

    #[serde_inline_default(false)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub minecraft_eula: bool,
}

fn is_default_registry(registry: &Url) -> bool {
    registry == &"https://creeper-registry.pages.dev/".parse().unwrap()
}

fn is_4(x: &usize) -> bool {
    *x == 4
}

fn is_8(x: &usize) -> bool {
    *x == 8
}

impl Default for Config {
    fn default() -> Self {
        Self {
            registry: "https://creeper-registry.pages.dev/".parse().unwrap(),
            parallel_job: 4,
            parallel_http: 8,
            use_bmclapi: false,
            minecraft_eula: false,
        }
    }
}

impl Creeper {
    pub(crate) async fn load_config(path: impl AsRef<Path>) -> anyhow::Result<Config> {
        let path = path.as_ref();

        if !path.exists() {
            info!("no config file at {}, using default", path.display());

            let config = Config::default();

            let toml = toml::to_string_pretty(&config)?;

            write(path, toml).await?;

            return Ok(config);
        }

        let toml = read_to_string(path).await?;

        let config = toml::from_str(&toml)?;

        Ok(config)
    }
}
