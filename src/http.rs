use reqwest::{IntoUrl, Response};

use crate::Creeper;

impl Creeper {
    pub(crate) async fn http_get(&self, url: impl IntoUrl + Send) -> anyhow::Result<Response> {
        let req = self.http.get(url).build()?;
        let res = self.http.execute(req).await?;
        Ok(res)
    }
}
