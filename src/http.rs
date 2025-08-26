use reqwest::{Client, IntoUrl, Response};

pub trait HttpRequest {
    fn http_get(
        &self,
        url: impl IntoUrl + Send,
    ) -> impl std::future::Future<Output = anyhow::Result<Response>> + Send;
}

impl<T> HttpRequest for T
where
    T: AsRef<Client> + Sync,
{
    async fn http_get(&self, url: impl IntoUrl + Send) -> anyhow::Result<Response> {
        let req = self.as_ref().get(url).build()?;
        let res = self.as_ref().execute(req).await?;
        Ok(res)
    }
}
