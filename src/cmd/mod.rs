use crate::Creeper;

pub mod run;

pub trait Execute {
    fn execute(self, lib: &Creeper)
    -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}
