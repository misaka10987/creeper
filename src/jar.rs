use std::{collections::HashMap, path::Path, str::FromStr};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use crate::zip::extract_zip;

/// Parse the first section of an RFC 822-like format.
///
/// # Note
///
/// TODO: this function does not yet guarantee complete support for the RFC 822 and there may exist behavioral difference in edge cases.
pub fn rfc822_first_section(s: &str) -> anyhow::Result<HashMap<&str, &str>> {
    let mut map = HashMap::new();

    let lines = s.lines().take_while(|l| !l.is_empty());

    for line in lines {
        let (key, value) = line.split_once(": ").ok_or(anyhow!("invalid line"))?;
        map.insert(key, value);
    }

    Ok(map)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct JarManifest {
    pub manifest_version: String,
    pub implementation_version: Option<String>,
    pub main_class: Option<String>,
}

impl FromStr for JarManifest {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let map = rfc822_first_section(s)?;

        let manifest_version = map
            .get("Manifest-Version")
            .ok_or(anyhow!("missing field Manifest-Version"))?
            .to_string();
        let implementation_version = map.get("Implementation-Version").map(|s| s.to_string());
        let main_class = map.get("Main-Class").map(|s| s.to_string());

        Ok(Self {
            manifest_version,
            implementation_version,
            main_class,
        })
    }
}

pub async fn jar_main_class(jar: impl AsRef<Path>) -> anyhow::Result<String> {
    let jar = jar.as_ref();

    let manifest = extract_zip(jar, "META-INF/MANIFEST.MF").await?;

    let manifest = manifest.parse::<JarManifest>()?;

    let main_class = manifest
        .main_class
        .ok_or(anyhow!("missing Main-Class in JAR META-INF/MANIFEST.MF"))?;

    Ok(main_class)
}
