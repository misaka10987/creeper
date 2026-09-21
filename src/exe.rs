use clap::Parser;
use tokio::runtime;
use tracing::level_filters::LevelFilter;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::{
    EnvFilter, Layer, fmt, layer::SubscriberExt, reload, util::SubscriberInitExt,
};

use crate::{Command, Creeper, inquire::make_filter};

pub fn execute() -> anyhow::Result<()> {
    let Command {
        args,
        cmd,
        log,
        log_level,
        tokio_console,
    } = Command::parse();

    let log_level = log_level.determine()?;

    let layer = IndicatifLayer::new();

    let (stdout, stderr) = (layer.get_stdout_writer(), layer.get_stderr_writer());

    let (filter, handle) = reload::Layer::new(make_filter(|_| true));

    if tokio_console {
        eprintln!("tokio-console enabled.");

        console_subscriber::init();
    } else {
        tracing_subscriber::registry()
            .with(EnvFilter::new(log))
            .with(LevelFilter::from_level(log_level))
            .with(
                fmt::layer()
                    .with_writer(layer.get_stderr_writer())
                    .with_filter(filter),
            )
            .with(layer)
            .init();
    }

    let run = runtime::Builder::new_multi_thread().enable_all().build()?;

    let creeper = run.block_on(Creeper::new(args))?;

    if !tokio_console {
        creeper.set_stdout(stdout);
        creeper.set_stderr(stderr);
        creeper.blocking_inquire_filter(handle);
    }

    run.block_on(creeper.execute(cmd))?;

    Ok(())
}
