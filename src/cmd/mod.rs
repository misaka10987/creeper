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
    fn execute(self, lib: &Creeper)
    -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}

/// Minecraft Package Manager.
#[derive(Clone, Debug, Parser)]
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

    /// Set the log filtering level.
    #[arg(name = "loglevel", long, default_value_t = Level::INFO)]
    pub log_level: Level,

    /// Use verbose output, equivalent to overriding log level to DEBUG.
    #[arg(short, long)]
    pub verbose: bool,

    /// Use noisy output, equivalent to overriding log level to TRACE.
    #[arg(short, long)]
    pub noisy: bool,

    #[command(subcommand)]
    pub cmd: SubCommand,
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
            SubCommand::AwwMan => Ok(println!("{CREEPER_TEXT_ART}")),
            SubCommand::Install(install) => lib.execute(install).await,
            SubCommand::Launch(launch) => lib.execute(launch).await,
            SubCommand::Nuke(nuke) => lib.execute(nuke).await,
            SubCommand::Login(login) => lib.execute(login).await,
            SubCommand::Init(init) => lib.execute(init).await,
            SubCommand::Add(add) => lib.execute(add).await,
            SubCommand::Dev(_dev) => todo!(),
            SubCommand::Complete(complete) => lib.execute(complete).await,
        }
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
