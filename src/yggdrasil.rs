use std::sync::OnceLock;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use tracing::info;
use url::Url;
use uuid::Uuid;

pub struct YggdrasilClient {
    pub server: Url,
    http: Client,
    api: OnceLock<Url>,
}

impl YggdrasilClient {
    pub fn new(server: Url, http: Client) -> Self {
        Self {
            server,
            http,
            api: OnceLock::new(),
        }
    }

    async fn api(&self) -> anyhow::Result<&Url> {
        if let Some(api) = self.api.get() {
            return Ok(api);
        }

        let res = self.http.get(self.server.clone()).send().await?;

        let api = if let Some(ali) = res.headers().get("X-Authlib-Injector-API-Location") {
            let new = self.server.join(ali.to_str()?)?;
            info!("following Yggdrasil ALI redirect: {} -> {new}", self.server);
            new
        } else {
            self.server.clone()
        };

        let api = self.api.get_or_init(|| api);

        Ok(api)
    }
}

#[serde_inline_default]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AuthRequest {
    pub username: String,
    pub password: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,

    #[serde_inline_default(false)]
    pub request_user: bool,

    /// Yggdrasil documentation does not document this.
    /// Use `Default::default()` for the field.
    pub agent: AuthRequestAgent,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AuthRequestAgent {
    pub name: String,
    pub version: u64,
}

impl Default for AuthRequestAgent {
    fn default() -> Self {
        Self {
            name: "Minecraft".into(),
            version: 1,
        }
    }
}

pub struct AuthResponse {
    pub access_token: String,
    pub client_token: String,
    pub available_profiles: Vec<Profile>,
    pub selected_profile: Option<Profile>,
    pub user: Option<Account>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Profile {
    #[serde(with = "uuid::serde::simple")]
    pub id: Uuid,
    pub name: String,
    pub properties: Vec<Property>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Property {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Account {
    #[serde(with = "uuid::serde::simple")]
    pub id: Uuid,
    pub properties: Vec<Property>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RefreshRequest {
    pub access_token: String,
    pub client_token: Option<String>,
    pub request_user: bool,
    pub selected_profile: Option<Profile>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RefreshResponse {
    pub access_token: String,
    pub client_token: String,
    pub selected_profile: Option<Profile>,
    pub user: Option<Account>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ValidateRequest {
    pub access_token: String,
    pub client_token: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InvalidateRequest {
    pub access_token: String,
    pub client_token: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SignoutRequest {
    pub username: String,
    pub password: String,
}
