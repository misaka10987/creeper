use anyhow::anyhow;
use clap::Parser;

use crate::{
    cmd::Execute,
    neoforge::{decode_neoforge_version, parse_neoforge_version},
};

/// Convert between NeoForge version numbers and semver.
#[derive(Clone, Debug, Parser)]
pub struct NeoForgeVersion {
    /// The version to convert in either NeoForge or semver format.
    #[arg(value_name = "VERSION")]
    version: String,
    /// To encode a NeoForge version to semver, otherwise decode.
    #[arg(long, default_value_t = false)]
    encode: bool,
}

impl Execute for NeoForgeVersion {
    async fn execute(self, _lib: &crate::Creeper) -> anyhow::Result<()> {
        if self.encode {
            let encoded = parse_neoforge_version(&self.version)
                .ok_or(anyhow!("unable to encode {} as semver", self.version))?;
            println!("{encoded}");
            return Ok(());
        }
        let decoded = decode_neoforge_version(&self.version.parse()?);
        println!("{decoded}");
        Ok(())
    }
}
