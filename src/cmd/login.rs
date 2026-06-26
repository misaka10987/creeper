use clap::Parser;
use colored::Colorize;

use crate::cmd::Execute;

/// Add a new Minecraft user account to the local configuration.
///
/// This will start an interactive CLI.
#[derive(Clone, Debug, Parser)]
pub struct Login;

impl Execute for Login {
    async fn execute(self, lib: &crate::Creeper) -> anyhow::Result<()> {
        let user = lib.prompt_new_user().await?;

        eprintln!("{} {user}", "Login".bold().green());

        Ok(())
    }
}
