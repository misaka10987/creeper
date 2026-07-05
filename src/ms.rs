use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use inquire::Text;
use oauth2::{
    AccessToken, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet,
    EndpointSet, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, Scope,
    TokenResponse, TokenUrl, basic::BasicClient, reqwest::redirect::Policy,
};
use tokio::{sync::RwLock, task::spawn_blocking};

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
    http: oauth2::reqwest::Client,
    oauth: OauthClient,
    access_token: RwLock<Option<AccessToken>>,
    refresh_token: RwLock<Option<RefreshToken>>,
    expires: RwLock<Option<u64>>,
    pkce_verifier: RwLock<Option<PkceCodeVerifier>>,
}

impl MicrosoftClient {
    pub fn new() -> anyhow::Result<Self> {
        let http = oauth2::reqwest::ClientBuilder::new()
            .redirect(Policy::none())
            .build()?;

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
        };

        Ok(value)
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

        eprintln!("{url}");

        let code = spawn_blocking(|| Text::new("Code:").prompt()).await??;

        let code = url::form_urlencoded::parse(code.as_bytes()).collect::<HashMap<_, _>>();

        let code = code["code"].to_string();

        let verifier = self.pkce_verifier.write().await.take().unwrap();

        let token = self
            .oauth
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(verifier)
            .request_async(&self.http)
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
}
