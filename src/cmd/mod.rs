use crate::Creeper;

pub mod add;
pub mod build_index;
pub mod complete;
pub mod init;
pub mod install;
pub mod launch;
pub mod login;
pub mod nuke;
mod prelude;

pub use prelude::*;

pub trait Execute {
    fn execute(self, lib: &Creeper)
    -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}
