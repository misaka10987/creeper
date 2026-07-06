use std::{
    fmt::Display,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, ensure};
use chrono::{DateTime, Utc};
use colored::Colorize;
use oauth2::{
    AccessToken, AuthUrl, AuthorizationCode, ClientId, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
    basic::BasicClient,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use tokio::sync::RwLock;
use tracing::error;
use url::Url;
use uuid::Uuid;

const AUTH_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";

const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";

const CLIENT_ID: &str = "2409a08e-df70-4e42-99ba-0843d4a1658e";

type OauthClient = oauth2::basic::BasicClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
>;

#[derive(Clone, Default, Serialize, Deserialize)]
struct Data {
    pub access_token: Option<AccessToken>,
    pub refresh_token: Option<RefreshToken>,
    pub access_token_expiry: Option<u64>,

    pub xbox_uhs: Option<String>,

    pub xbl_token: Option<String>,
    pub xbl_token_expiry: Option<chrono::DateTime<Utc>>,

    pub xsts_token: Option<String>,
    pub xsts_token_expiry: Option<chrono::DateTime<Utc>>,

    pub mc_jwt: Option<String>,
    pub mc_jwt_expiry: Option<u64>,

    pub mc_uuid: Option<Uuid>,
    pub mc_name: Option<String>,
}

pub struct MicrosoftClient {
    http: Client,
    oauth: OauthClient,
    data: RwLock<Data>,
}

impl MicrosoftClient {
    pub fn new(http: Client) -> anyhow::Result<Self> {
        let oauth = BasicClient::new(ClientId::new(CLIENT_ID.into()))
            // .set_client_secret(ClientSecret::new("secret".into()))
            .set_auth_uri(AuthUrl::new(AUTH_URL.into())?)
            .set_token_uri(TokenUrl::new(TOKEN_URL.into())?)
            .set_redirect_uri(RedirectUrl::new("http://localhost:5555".into())?);

        let value = Self {
            http,
            oauth,
            data: RwLock::new(Default::default()),
        };

        Ok(value)
    }

    /// Whether the Microsoft access token is within expiry time.
    ///
    /// Note that this function returns **`true`** if there is no expiry time. This is to better indicate whether a token refresh will be needed.
    pub async fn access_token_expired(&self) -> bool {
        let data = self.data.read().await;

        let expiry = match data.access_token_expiry {
            Some(time) => time,
            None => return true,
        };

        let expiry = SystemTime::UNIX_EPOCH + Duration::from_secs(expiry);

        SystemTime::now() + Duration::from_secs(15 * 60) >= expiry
    }

    pub async fn prompt_login(&self) -> anyhow::Result<()> {
        let mut data = self.data.write().await;

        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();

        let (url, csrf) = self
            .oauth
            .authorize_url(CsrfToken::new_random)
            .add_scopes([
                Scope::new("XboxLive.signin".into()),
                Scope::new("offline_access".into()),
            ])
            .set_pkce_challenge(challenge)
            .url();

        let redirect = local_redirect_uri::Server::new(5555, csrf.into_secret());

        eprintln!("{} {url}", "Open".bold().cyan());

        open::that_detached(url.as_str())?;

        let code = redirect.wait_code().await?;

        let http = oauth2::reqwest::ClientBuilder::new()
            .redirect(oauth2::reqwest::redirect::Policy::none())
            .build()?;

        let token = self
            .oauth
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(verifier)
            .request_async(&http)
            .await?;

        data.access_token = Some(token.access_token().clone());

        data.refresh_token = token.refresh_token().cloned();

        data.access_token_expiry = token.expires_in().map(|x| calc_expiry(x.as_secs()));

        Ok(())
    }

    pub async fn xbox_auth(&self) -> anyhow::Result<()> {
        const URL: &str = "https://user.auth.xboxlive.com/user/authenticate";

        let mut data = self.data.write().await;

        let access_token = data
            .access_token
            .clone()
            .ok_or(anyhow!("no Microsoft OAuth access_token present"))?;

        let req = XboxAuthRequest::new(access_token.into_secret());

        let res = self
            .http
            .post(URL)
            .json(&req)
            .send()
            .await?
            .error_for_status()?
            .json::<XboxAuthResponse>()
            .await?;

        data.xbox_uhs = Some(res.display_claims.uhs()?);

        data.xbl_token = Some(res.token);

        data.xbl_token_expiry = Some(res.not_after);

        Ok(())
    }

    pub async fn xsts_auth(&self) -> anyhow::Result<()> {
        const URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";

        let mut data = self.data.write().await;

        let xbl_token = data
            .xbl_token
            .clone()
            .ok_or(anyhow!("no Xbox Live token present"))?;

        let uhs = data.xbox_uhs.as_ref().ok_or(anyhow!("no Xbox user hash"))?;

        let req = XstsAuthRequest::new(xbl_token);

        let res = self
            .http
            .post(URL)
            .json(&req)
            .send()
            .await?
            .error_for_status()?
            .json::<XboxAuthResponse>()
            .await?;

        ensure!(*uhs == res.display_claims.uhs()?, "Xbox user hash mismatch");

        data.xsts_token = Some(res.token);
        data.xsts_token_expiry = Some(res.not_after);

        Ok(())
    }

    pub async fn xbl_token_expired(&self) -> bool {
        let data = self.data.read().await;

        let expiry = match data.xbl_token_expiry {
            Some(time) => time,
            None => return true,
        };

        Utc::now() >= expiry
    }

    pub async fn xsts_token_expired(&self) -> bool {
        let data = self.data.read().await;

        let expiry = match data.xsts_token_expiry {
            Some(time) => time,
            None => return true,
        };

        Utc::now() >= expiry
    }

    pub async fn mc_login(&self) -> anyhow::Result<()> {
        const URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";

        let mut data = self.data.write().await;

        let uhs = data.xbox_uhs.as_ref().ok_or(anyhow!("no Xbox user hash"))?;

        let xsts_token = data
            .xsts_token
            .as_ref()
            .ok_or(anyhow!("no XSTS token present"))?;

        let req = McLoginRequest::new(uhs, xsts_token);

        let res = self
            .http
            .post(URL)
            .json(&req)
            .send()
            .await?
            .error_for_status()?
            .json::<McLoginResponse>()
            .await?;

        data.mc_jwt = Some(res.access_token);
        data.mc_jwt_expiry = Some(calc_expiry(res.expires_in));

        Ok(())
    }

    pub async fn owns_minecraft(&self) -> anyhow::Result<bool> {
        const URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";

        let data = self.data.read().await;

        let token = data
            .mc_jwt
            .as_ref()
            .ok_or(anyhow!("no Minecraft JWT present"))?;

        let res = self
            .http
            .get(URL)
            .bearer_auth(token)
            .send()
            .await?
            .error_for_status()?
            .json::<McStoreResponse>()
            .await?;

        Ok(!res.items.is_empty())
    }

    pub async fn get_mc_user(&self) -> anyhow::Result<(Uuid, String)> {
        const URL: &str = "https://api.minecraftservices.com/minecraft/profile";

        let mut data = self.data.write().await;

        if let (Some(uuid), Some(name)) = (data.mc_uuid, data.mc_name.clone()) {
            return Ok((uuid, name));
        }

        let token = data
            .mc_jwt
            .as_ref()
            .ok_or(anyhow!("no Minecraft JWT present"))?;

        let res = self
            .http
            .get(URL)
            .bearer_auth(token)
            .send()
            .await?
            .error_for_status()?
            .json::<McProfileResponse>()
            .await?;

        data.mc_uuid = Some(res.id);
        data.mc_name = Some(res.name.clone());

        Ok((res.id, res.name))
    }
}

#[serde_inline_default]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "PascalCase")]
pub struct XboxAuthRequest {
    pub properties: xbox::Properties,

    #[serde_inline_default("http://auth.xboxlive.com".into())]
    pub relying_party: String,

    #[serde_inline_default("JWT".into())]
    pub token_type: String,
}

impl XboxAuthRequest {
    pub fn new(access_token: String) -> Self {
        Self {
            properties: xbox::Properties::new(access_token),
            relying_party: "http://auth.xboxlive.com".into(),
            token_type: "JWT".into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "PascalCase")]
pub struct XboxAuthResponse {
    pub issue_instant: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub token: String,
    pub display_claims: xbox::DisplayClaims,
}

pub mod xbox {
    use anyhow::ensure;
    use serde::{Deserialize, Serialize};
    use serde_inline_default::serde_inline_default;

    #[serde_inline_default]
    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "PascalCase")]
    pub struct Properties {
        #[serde_inline_default("RPS".into())]
        pub auth_method: String,

        #[serde_inline_default("user.auth.xboxlive.com".into())]
        pub site_name: String,

        pub rps_ticket: String,
    }

    impl Properties {
        pub fn new(access_token: String) -> Self {
            Self {
                auth_method: "RPS".into(),
                site_name: "user.auth.xboxlive.com".into(),
                rps_ticket: format!("d={access_token}"),
            }
        }
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct DisplayClaims {
        pub xui: Vec<Xui>,
    }

    impl DisplayClaims {
        pub fn uhs(self) -> anyhow::Result<String> {
            ensure!(self.xui.len() == 1, "multiple user hash returned");

            let xui = self.xui.into_iter().next().unwrap();

            Ok(xui.uhs)
        }
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Xui {
        pub uhs: String,
    }
}

#[serde_inline_default]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "PascalCase")]
pub struct XstsAuthRequest {
    pub properties: xsts::Properties,

    #[serde_inline_default("rp://api.minecraftservices.com/".parse().unwrap())]
    pub relying_party: Url,

    #[serde_inline_default("JWT".into())]
    pub token_type: String,
}

impl XstsAuthRequest {
    pub fn new(xbl_token: String) -> Self {
        Self {
            properties: xsts::Properties::new(xbl_token),
            relying_party: "rp://api.minecraftservices.com/".parse().unwrap(),
            token_type: "JWT".into(),
        }
    }
}

pub mod xsts {
    use serde::{Deserialize, Serialize};
    use serde_inline_default::serde_inline_default;

    #[serde_inline_default]
    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "PascalCase")]
    pub struct Properties {
        #[serde_inline_default("RETAIL".into())]
        pub sandbox_id: String,

        pub user_tokens: Vec<String>,
    }

    impl Properties {
        pub fn new(xbl_token: String) -> Self {
            Self {
                sandbox_id: "RETAIL".into(),
                user_tokens: vec![xbl_token],
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McLoginRequest {
    pub identity_token: String,
}

impl McLoginRequest {
    pub fn new(uhs: impl Display, xsts_token: impl Display) -> Self {
        Self {
            identity_token: format!("XBL3.0 x={uhs};{xsts_token}"),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McLoginResponse {
    pub username: String,
    pub roles: Vec<String>,
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
}

fn calc_expiry(expires_in: u64) -> u64 {
    (SystemTime::now() + Duration::from_secs(expires_in))
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McStoreResponse {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<mc::Item>,

    pub signature: String,

    pub key_id: String,
}

pub mod mc {
    use serde::{Deserialize, Serialize};
    use url::Url;
    use uuid::Uuid;

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Item {
        pub name: String,
        pub signature: String,
    }

    #[derive(Clone, Serialize, Deserialize)]
    pub struct Skin {
        pub id: Uuid,
        pub state: String,
        pub url: Url,
        pub variant: String,
        pub alias: String,
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McProfileResponse {
    #[serde(with = "uuid::serde::simple")]
    pub id: Uuid,

    pub name: String,

    pub skins: Vec<mc::Skin>,

    pub capes: Vec<serde_json::Value>,
}
