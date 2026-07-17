use crate::Creeper;

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
