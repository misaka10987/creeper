use clap::{CommandFactory, Parser, ValueEnum};
use clap_complete::{
    Shell::{Bash, Elvish, Fish, PowerShell, Zsh},
    generate,
};
use clap_complete_nushell::Nushell;
use parse_display::{Display, FromStr};

use crate::{Args, cmd::Execute};

/// Generate shell completions.
#[derive(Clone, Debug, Parser)]
pub struct Complete {
    #[arg(value_name = "SHELL")]
    pub shell: Shell,
}

#[derive(Clone, Debug, Display, FromStr, ValueEnum)]
#[display(style = "lowercase")]
pub enum Shell {
    #[value(name = "bash")]
    Bash,

    #[value(name = "elvish")]
    Elvish,

    #[value(name = "fish")]
    Fish,

    #[value(name = "nushell")]
    NuShell,

    #[value(name = "powershell")]
    PowerShell,

    #[value(name = "zsh")]
    Zsh,
}

impl Execute for Complete {
    async fn execute(self, _lib: &crate::Creeper) -> anyhow::Result<()> {
        let mut cmd = Args::command();

        let mut buf = vec![];

        match self.shell {
            Shell::Bash => generate(Bash, &mut cmd, "creeper", &mut buf),
            Shell::Elvish => generate(Elvish, &mut cmd, "creeper", &mut buf),
            Shell::Fish => generate(Fish, &mut cmd, "creeper", &mut buf),
            Shell::NuShell => generate(Nushell, &mut cmd, "creeper", &mut buf),
            Shell::PowerShell => generate(PowerShell, &mut cmd, "creeper", &mut buf),
            Shell::Zsh => generate(Zsh, &mut cmd, "creeper", &mut buf),
        }

        let script = String::from_utf8(buf)?;

        println!("{script}");

        Ok(())
    }
}
