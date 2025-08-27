use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Artifact;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lock {
    #[serde(rename = "deployment")]
    pub deploy: Vec<Deployment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
