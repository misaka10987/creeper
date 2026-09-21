use clap::Parser;

use crate::cmd::Execute;

/// CLI for development use.
///
/// Note that this interface is subject to change and any behavior shall not be depended on.
#[derive(Clone, Debug, Parser)]
pub enum Dev {
    LogTest,
    NOP,
}

impl Execute for Dev {
    async fn execute(self, _lib: &crate::Creeper) -> anyhow::Result<()> {
        match self {
            Dev::LogTest => {
                log_test();
                Ok(())
            }
            Dev::NOP => Ok(()),
        }
    }
}

fn log_test() {
    mod foo {
        use tracing::{debug, error, info, trace, warn};

        pub fn log() {
            trace!("foo trace");
            debug!("foo debug");
            info!("foo info");
            warn!("foo warn");
            error!("foo error");
        }
    }

    mod bar {
        use tracing::{debug, error, info, trace, warn};

        pub fn log() {
            trace!("bar trace");
            debug!("bar debug");
            info!("bar info");
            warn!("bar warn");
            error!("bar error");
        }
    }

    foo::log();
    bar::log();
}
