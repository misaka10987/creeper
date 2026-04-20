use std::{collections::BTreeSet, str::FromStr, sync::OnceLock};

use reqwest::Client;
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::Creeper;

const VERSIONS_URL: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

pub struct NeoforgeManager {
    http: Client,
    versions: OnceLock<BTreeSet<Version>>,
}

impl NeoforgeManager {
    pub fn new(http: Client) -> Self {
        Self {
            http,
            versions: OnceLock::new(),
        }
    }

    pub async fn list_version(&self) -> anyhow::Result<&BTreeSet<Version>> {
        if let Some(versions) = self.versions.get() {
            return Ok(versions);
        }

        let req = self.http.get(VERSIONS_URL).build()?;
        let res = self.http.execute(req).await?;

        #[derive(Clone, Debug, Serialize, Deserialize)]
        struct Versions {
            #[serde(rename = "isSnapshot")]
            is_snapshot: bool,
            versions: Vec<String>,
        }

        let versions = res.json::<Versions>().await?;

        let versions = versions
            .versions
            .into_iter()
            .filter_map(|v| Version::from_str(&v).ok());

        let versions = versions.into_iter().collect();

        Ok(self.versions.get_or_init(|| versions))
    }
}

impl Creeper {
    pub async fn list_neoforge_version(&self) -> anyhow::Result<&BTreeSet<Version>> {
        self.neoforge.list_version().await
    }
}
