use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Artifact;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    pub path: PathBuf,
    #[serde(flatten)]
    pub artifact: Artifact,
}
