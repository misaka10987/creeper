use std::time::{Duration, SystemTime};

use colored::Colorize;
use oauth2::{
    AccessToken, AuthorizationCode, CsrfToken, PkceCodeChallenge, RefreshToken, Scope,
    TokenResponse,
};
use tracing::{info, trace};

use crate::ms::{MicrosoftClient, calc_expiry};

impl MicrosoftClient {
    fn ms_scopes() -> Vec<Scope> {
        vec![
            Scope::new("XboxLive.signin".into()),
            Scope::new("offline_access".into()),
        ]
    }

    fn http_oauth() -> anyhow::Result<oauth2::reqwest::Client> {
        let client = oauth2::reqwest::ClientBuilder::new()
            .redirect(oauth2::reqwest::redirect::Policy::none())
            .build()?;

        Ok(client)
    }

    pub async fn get_ms_access_token(&self) -> anyhow::Result<AccessToken> {
        if let Some(token) = &self.data.read().await.access_token
            && !self.ms_access_token_expired().await
        {
            trace!("Microsoft session is valid, using cached access_token");

            return Ok(token.clone());
        }

        info!("Microsoft session is invalid, refreshing");

        self.refresh_ms_token().await?;

        let token = self.data.read().await.access_token.clone().unwrap();

        Ok(token)
    }

    pub async fn get_ms_refresh_token(&self) -> anyhow::Result<RefreshToken> {
        if let Some(token) = &self.data.read().await.refresh_token {
            trace!("using cached Microsoft OAuth refresh_token");

            return Ok(token.clone());
        }

        info!("no Microsoft OAuth refresh_token, prompting login");

        self.prompt_login().await?;

        let token = self.data.read().await.refresh_token.clone().unwrap();

        Ok(token)
    }

    pub async fn refresh_ms_token(&self) -> anyhow::Result<()> {
        let mut data = self.data.write().await;

        let refresh = self.get_ms_refresh_token().await?;

        let token = self
            .oauth
            .exchange_refresh_token(&refresh)
            .add_scopes(Self::ms_scopes())
            .request_async(&Self::http_oauth()?)
            .await?;

        data.access_token = Some(token.access_token().clone());

        data.refresh_token = token.refresh_token().cloned();

        data.access_token_expiry = token.expires_in().map(|x| calc_expiry(x.as_secs()));

        Ok(())
    }

    /// Whether the Microsoft access token is within expiry time.
    ///
    /// Note that this function returns **`true`** if there is no expiry time. This is to better indicate whether a token refresh will be needed.
    pub async fn ms_access_token_expired(&self) -> bool {
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

        info!("logged in, proceeding to PKCE token exchange");

        let token = self
            .oauth
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(verifier)
            .request_async(&Self::http_oauth()?)
            .await?;

        data.access_token = Some(token.access_token().clone());

        data.refresh_token = token.refresh_token().cloned();

        data.access_token_expiry = token.expires_in().map(|x| calc_expiry(x.as_secs()));

        Ok(())
    }
}
