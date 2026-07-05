use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, ensure};
use chrono::{DateTime, Utc};
use colored::Colorize;
use oauth2::{
    AccessToken, AuthUrl, AuthorizationCode, ClientId, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
    basic::BasicClient,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use tokio::sync::RwLock;
use url::Url;

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

pub struct MicrosoftClient {
    http: Client,
    oauth: OauthClient,
    access_token: RwLock<Option<AccessToken>>,
    refresh_token: RwLock<Option<RefreshToken>>,
    expires: RwLock<Option<u64>>,
    pkce_verifier: RwLock<Option<PkceCodeVerifier>>,

    xbox_uhs: RwLock<Option<String>>,
    xbox_token: RwLock<Option<String>>,
    xbox_token_expiry: RwLock<Option<chrono::DateTime<Utc>>>,

    xsts_token: RwLock<Option<String>>,
    xsts_token_expiry: RwLock<Option<chrono::DateTime<Utc>>>,
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
            access_token: RwLock::new(None),
            refresh_token: RwLock::new(None),
            expires: RwLock::new(None),
            pkce_verifier: RwLock::new(None),
            xbox_uhs: RwLock::new(None),
            xbox_token: RwLock::new(None),
            xbox_token_expiry: RwLock::new(None),
            xsts_token: RwLock::new(None),
            xsts_token_expiry: RwLock::new(None),
        };

        Ok(value)
    }

    /// Whether the Microsoft access token is within expiry time.
    ///
    /// Note that this function returns **`true`** if there is no expiry time. This is to better indicate whether a token refresh will be needed.
    pub async fn access_token_expired(&self) -> bool {
        let expiry = self.expires.read().await;

        let expiry = match *expiry {
            Some(time) => time,
            None => return true,
        };

        let expiry = SystemTime::UNIX_EPOCH + Duration::from_secs(expiry);

        expiry >= SystemTime::now() + Duration::from_secs(15 * 60)
    }

    pub async fn prompt_login(&self) -> anyhow::Result<()> {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();

        *self.pkce_verifier.write().await = Some(verifier);

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

        let verifier = self.pkce_verifier.write().await.take().unwrap();

        let http = oauth2::reqwest::ClientBuilder::new()
            .redirect(oauth2::reqwest::redirect::Policy::none())
            .build()?;

        let token = self
            .oauth
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(verifier)
            .request_async(&http)
            .await?;

        *self.access_token.write().await = Some(token.access_token().clone());

        *self.refresh_token.write().await = token.refresh_token().cloned();

        *self.expires.write().await = token.expires_in().map(|x| {
            (SystemTime::now() + x)
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        });

        Ok(())
    }

    pub async fn xbox_auth(&self) -> anyhow::Result<()> {
        const URL: &str = "https://user.auth.xboxlive.com/user/authenticate";

        let access_token = self
            .access_token
            .read()
            .await
            .clone()
            .ok_or(anyhow!("no Microsoft OAuth access_token present"))?;

        let req = XboxAuthRequest::new(access_token.into_secret());

        let res = self
            .http
            .post(URL)
            .json(&req)
            .send()
            .await?
            .json::<XboxAuthResponse>()
            .await?;

        *self.xbox_uhs.write().await = Some(res.display_claims.uhs()?);

        *self.xbox_token.write().await = Some(res.token);

        *self.xbox_token_expiry.write().await = Some(res.not_after);

        Ok(())
    }

    pub async fn xsts_auth(&self) -> anyhow::Result<()> {
        const URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";

        let xbl_token = self
            .xbox_token
            .read()
            .await
            .clone()
            .ok_or(anyhow!("no Xbox Live token present"))?;

        let uhs = self.xbox_uhs.read().await;
        let uhs = uhs.as_ref().ok_or(anyhow!("no Xbox user hash"))?;

        let req = XstsAuthRequest::new(xbl_token);

        let res = self
            .http
            .post(URL)
            .json(&req)
            .send()
            .await?
            .json::<XboxAuthResponse>()
            .await?;

        ensure!(*uhs == res.display_claims.uhs()?, "Xbox user hash mismatch");

        *self.xsts_token.write().await = Some(res.token);
        *self.xbox_token_expiry.write().await = Some(res.not_after);

        Ok(())
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
                rps_ticket: access_token,
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
