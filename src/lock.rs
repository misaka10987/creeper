use std::collections::HashMap;

use semver::VersionReq;
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};
use url::Url;

use crate::{Id, index::VersionRev};

#[serde_as]
#[derive(Clone, Serialize, Deserialize)]
pub struct Lock {
    pub registry: Url,
    #[serde_as(as = "HashMap<_, DisplayFromStr>")]
    pub package: HashMap<Id, VersionRev>,
}

impl Lock {
    pub fn satisfies(&self, req: impl IntoIterator<Item = (Id, VersionReq)>) -> bool {
        for (id, req) in req {
            if !self
                .package
                .get(&id)
                .is_some_and(|v| req.matches(&v.version))
            {
                return false;
            }
        }
        true
    }
}
