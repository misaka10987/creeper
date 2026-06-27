use anyhow::bail;
use clap::Parser;

use crate::{
    cmd::{self, Execute},
    id::IdVersionReq,
};

/// Add dependencies to the current game instance.
#[derive(Clone, Debug, Parser)]
pub struct Add {
    /// The dependencies to add.
    #[arg(value_name = "<PACKAGE>[@<VERSION_REQ>]", required = true)]
    pub req: Vec<IdVersionReq>,

    /// Whether to override existing dependencies in the manifest file.
    #[arg(short = 'r', long = "override")]
    pub overwrite: bool,

    /// Whether to run `creeper install` after adding the dependencies.
    #[arg(short, long, default_value_t = true)]
    pub install: bool,
}

impl Execute for Add {
    async fn execute(self, lib: &crate::Creeper) -> anyhow::Result<()> {
        let mut pack = lib.game_pack().await?;

        for IdVersionReq { id, version_req } in self.req {
            if let Some(exist) = pack.node.dep.insert(id.clone(), version_req.clone()) {
                if !self.overwrite {
                    bail!(
                        "cannot add {id}@{version_req}: {id}@{exist} already exists in the manifest, use --override to override"
                    );
                }
            }
        }

        lib.set_game_pack(pack).await?;

        let install = cmd::Install { update: true };

        lib.execute(install).await?;

        Ok(())
    }
}
