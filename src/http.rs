use reqwest::{Client, IntoUrl, Response};

#[allow(async_fn_in_trait)]
pub trait HttpRequest {
    async fn http_get(&self, url: impl IntoUrl) -> anyhow::Result<Response>;
}

impl<T: AsRef<Client>> HttpRequest for T {
    async fn http_get(&self, url: impl IntoUrl) -> anyhow::Result<Response> {
        let req = self.as_ref().get(url).build()?;
        let res = self.as_ref().execute(req).await?;
        Ok(res)
    }
}
