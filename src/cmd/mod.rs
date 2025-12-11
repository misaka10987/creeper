use crate::Creeper;

pub mod run;

pub trait Execute {
    fn execute(
        lib: &Creeper,
        cmd: Self,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}
