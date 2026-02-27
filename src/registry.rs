use url::Url;

pub struct Registry {
    pub url: Url,
}

impl Registry {
    pub fn new(url: Url) -> Self {
        Self { url }
    }
}
