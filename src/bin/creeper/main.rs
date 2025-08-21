use clap::Parser;
use creeper::{Creeper, CreeperArgs};
use stop::stop;
use tokio::runtime;

fn main() {
    let args = CreeperArgs::parse();
    let creeper = Creeper::new(args);
    let run = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(stop!());
    run.block_on(creeper.run());
}
