use std::{
    fmt::Display,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, ensure};
use inquire::Select;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_with::{NoneAsEmptyString, serde_as};
use tokio::{process::Command, task::spawn_blocking};
use tracing::debug;

use crate::{Creeper, path::creeper_config_dir, util::TomlFile};

pub struct JavaManager {
    pub config: TomlFile<JavaConfig>,
}

impl JavaManager {
    pub fn new() -> Self {
        Self {
            config: TomlFile::new(),
        }
    }
}

fn config_path() -> anyhow::Result<PathBuf> {
    let path = creeper_config_dir()?.join("java.toml");

    Ok(path)
}

impl Creeper {
    pub async fn prompt_select_java(&self, req: &VersionReq) -> anyhow::Result<Java> {
        let path = config_path()?;

        let config = self.java.config.read(&path).await?.unwrap_or_default();

        let all = [Java::path().await?]
            .into_iter()
            .chain(config.java)
            .filter(|v| req.matches(&v.version))
            .collect::<Vec<_>>();

        ensure!(!all.is_empty(), "no configured Java runtime {req}");

        if all.len() == 1 {
            debug!("using the only java runtime matching {req}: {}", all[0]);

            return Ok(all.into_iter().next().unwrap());
        }

        let select = spawn_blocking(|| {
            Select::new("Select a Java runtime:", all)
                .with_starting_cursor(0)
                .prompt()
        })
        .await??;

        Ok(select)
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct JavaConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub java: Vec<Java>,
}

#[serde_as]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Java {
    #[serde_as(as = "NoneAsEmptyString")]
    pub name: Option<String>,

    pub version: Version,

    pub path: PathBuf,
}

impl Display for Java {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(name) = &self.name {
            write!(f, "({}) {name}", self.version)
        } else {
            write!(f, "({}) {}", self.version, self.path.display())
        }
    }
}

impl Java {
    pub async fn path() -> anyhow::Result<Self> {
        let path = PathBuf::from("java");

        let version = get_java_version(&path).await?;

        let value = Self {
            name: Some("$PATH".into()),
            version,
            path,
        };

        Ok(value)
    }

    pub async fn check_version(&self) -> anyhow::Result<bool> {
        let version = get_java_version(&self.path).await?;

        Ok(version == self.version)
    }
}

async fn get_java_version(bin: impl AsRef<Path>) -> anyhow::Result<Version> {
    let mut cmd = Command::new(bin.as_ref());
    cmd.arg("--version");

    let output = cmd.output().await?;

    let output = String::from_utf8_lossy(&output.stdout);

    let lines = output.lines().collect::<Vec<_>>();

    ensure!(lines.len() >= 1, "java --version output is empty");

    let line = lines[0];

    let version = line
        .strip_prefix("java")
        .ok_or(anyhow!("invalid java --version output: {line}"))?
        .split_whitespace()
        .next()
        .ok_or(anyhow!("invalid java --version output: {line}"))?
        .parse()?;

    Ok(version)
}
