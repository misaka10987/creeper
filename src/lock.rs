use std::path::PathBuf;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use crate::{Artifact, Inst, Install, creeper_minecraft};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Lock {
    #[serde(rename = "config")]
    pub cfg: Inst,
    pub java_main_class: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub java_flag: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mc_flag: Vec<String>,
    #[serde(rename = "deployment", default, skip_serializing_if = "Vec::is_empty")]
    pub deploy: Vec<Deployment>,
}

pub struct LockBuilder {
    cfg: Inst,
    install: Install,
}

impl LockBuilder {
    pub fn new(cfg: Inst) -> Self {
        Self {
            cfg,
            install: Default::default(),
        }
    }

    pub fn build(self) -> anyhow::Result<Lock> {
        let Install {
            java_lib,
            java_main_class,
            native,
            java_flag,
            mc_jar,
            mut mc_flag,
            mc_asset_index,
            mc_mod,
        } = self.install;

        let mut deploy = vec![];

        deploy.extend(
            java_lib
                .into_iter()
                .enumerate()
                .map(|(n, a)| (format!("libraries/{n}"), a).into()),
        );

        deploy.extend(native.into_iter().map(|x| x.into()));

        deploy.push(
            (
                "minecraft.jar",
                mc_jar.ok_or(anyhow!("minecraft.jar unspecified"))?,
            )
                .into(),
        );

        deploy.extend(
            mc_mod
                .into_iter()
                .enumerate()
                .map(|(n, a)| (format!("mods/{n}"), a).into()),
        );

        let mc_asset_index = mc_asset_index.ok_or(anyhow!("minecraft asset index unspecified"))?;
        mc_flag.extend(vec!["--assetIndex".into(), mc_asset_index.blake3.clone()]);
        deploy.push(
            (
                creeper_minecraft()?
                    .join("assets")
                    .join("indexes")
                    .join(mc_asset_index.blake3.clone()),
                mc_asset_index,
            )
                .into(),
        );

        let val = Lock {
            cfg: self.cfg,
            java_main_class: java_main_class.ok_or(anyhow!("java main class unspecified"))?,
            java_flag,
            mc_flag,
            deploy,
        };
        Ok(val)
    }
}

impl Extend<Install> for LockBuilder {
    fn extend<T: IntoIterator<Item = Install>>(&mut self, iter: T) {
        self.install.extend(iter);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Deployment {
    pub path: PathBuf,
    #[serde(flatten)]
    pub artifact: Artifact,
}

impl<T: Into<PathBuf>> From<(T, Artifact)> for Deployment {
    fn from(value: (T, Artifact)) -> Self {
        let (path, artifact) = value;
        Self {
            path: path.into(),
            artifact,
        }
    }
}
