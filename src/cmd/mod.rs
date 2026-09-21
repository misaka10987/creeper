use std::io::Write;

use anyhow::bail;
use clap::Parser;
use tracing::Level;

use crate::{Args, Creeper, dev::Dev, tool::Tool};

mod add;
mod complete;
mod init;
mod install;
mod launch;
mod login;
mod nuke;
mod prelude;

pub use prelude::*;

pub trait Execute {
    fn execute(self, lib: &Creeper) -> impl std::future::Future<Output = anyhow::Result<()>>;
}

/// Minecraft Package Manager.
#[derive(Clone, Parser)]
#[command(version)]
pub struct Command {
    #[clap(flatten)]
    pub args: Args,

    /// The log filtering directives.
    ///
    /// This is independent of the `--loglevel` option.
    /// See https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#directives for syntax.
    #[arg(long, default_value = "trace,creeper_pubgrub=warn")]
    pub log: String,

    #[clap(flatten)]
    pub log_level: LogLevel,

    #[command(subcommand)]
    pub cmd: SubCommand,
}

#[derive(Clone, Parser)]
pub struct LogLevel {
    /// Set the log filtering level.
    #[arg(name = "loglevel", long, default_value_t = Level::INFO)]
    log_level: Level,

    /// Use verbose output, equivalent to overriding log level to DEBUG.
    #[arg(short, long)]
    verbose: bool,

    /// Use noisy output, equivalent to overriding log level to TRACE.
    #[arg(short, long)]
    noisy: bool,

    /// Use quiet output, equivalent to overriding log level to ERROR.
    #[arg(short, long)]
    quiet: bool,
}

impl LogLevel {
    pub fn determine(self) -> anyhow::Result<Level> {
        if [
            self.log_level != Level::INFO,
            self.verbose,
            self.noisy,
            self.quiet,
        ]
        .into_iter()
        .filter(|x| *x)
        .count()
            > 1
        {
            bail!("paradoxical log level arguments")
        }

        let level = if self.noisy {
            Level::TRACE
        } else if self.verbose {
            Level::DEBUG
        } else if self.quiet {
            Level::ERROR
        } else {
            self.log_level
        };

        Ok(level)
    }
}

#[derive(Clone, Debug, Parser)]
pub enum SubCommand {
    #[command(subcommand)]
    Tool(Tool),

    Add(Add),

    Launch(Launch),

    Install(Install),

    Nuke(Nuke),

    Login(Login),

    Init(Init),

    #[command(subcommand, hide = true)]
    Dev(Dev),

    Complete(Complete),

    #[clap(hide = true)]
    AwwMan,
}

impl Execute for SubCommand {
    async fn execute(self, lib: &Creeper) -> anyhow::Result<()> {
        match self {
            SubCommand::Tool(tool) => lib.execute(tool).await,
            SubCommand::AwwMan => Ok(writeln!(lib.get_stdout(), "{CREEPER_TEXT_ART}").unwrap()),
            SubCommand::Install(install) => lib.execute(install).await,
            SubCommand::Launch(launch) => lib.execute(launch).await,
            SubCommand::Nuke(nuke) => lib.execute(nuke).await,
            SubCommand::Login(login) => lib.execute(login).await,
            SubCommand::Init(init) => lib.execute(init).await,
            SubCommand::Add(add) => lib.execute(add).await,
            SubCommand::Dev(dev) => lib.execute(dev).await,
            SubCommand::Complete(complete) => lib.execute(complete).await,
        }
    }
}

impl Creeper {
    pub async fn execute(&self, cmd: impl Execute) -> anyhow::Result<()> {
        cmd.execute(self).await
    }
}

const CREEPER_TEXT_ART: &str = r#"
🟩🟩🟩⬜⬜🟩🟩🟩
🟩🟩🟩🟩🟩🟩🟩⬜
🟩⬛⬛🟩🟩⬛⬛⬜
🟩⬛⬛🟩🟩⬛⬛🟩
🟩🟩🟩⬛⬛⬜🟩🟩
🟩🟩⬛⬛⬛⬛🟩⬜
⬜🟩⬛⬛⬛⬛🟩🟩
🟩🟩⬛🟩🟩⬛🟩🟩
"#;
