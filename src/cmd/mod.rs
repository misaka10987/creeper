pub mod run;

pub trait Execute<T> {
    fn execute(&self, cmd: T) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}
