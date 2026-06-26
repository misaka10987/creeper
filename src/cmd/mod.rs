use crate::Creeper;

pub mod build_index;
pub mod download;
pub mod init;
pub mod install;
pub mod launch;
pub mod login;
pub mod nf_version;
pub mod nuke;

pub trait Execute {
    fn execute(self, lib: &Creeper)
    -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}
