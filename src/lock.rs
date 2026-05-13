use std::collections::HashMap;

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::Id;

#[derive(Clone, Serialize, Deserialize)]
pub struct Lock {
    pub registry: Url,
    pub package: HashMap<Id, Version>,
}

impl Lock {
    pub fn satisfies(&self, req: impl IntoIterator<Item = (Id, VersionReq)>) -> bool {
        for (id, req) in req {
            if !self.package.get(&id).is_some_and(|v| req.matches(v)) {
                return false;
            }
        }
        true
    }
}
