use anyhow::{anyhow, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use tracing::{info, trace};
use url::Url;

use crate::ms::MicrosoftClient;

impl MicrosoftClient {
    pub async fn xbox_auth(&self) -> anyhow::Result<()> {
        const URL: &str = "https://user.auth.xboxlive.com/user/authenticate";

        let access_token = self.get_ms_access_token().await?;

        let mut data = self.data.write().await;

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

        let uhs = res.display_claims.uhs()?;

        info!("authenticated with Xbox Live (user hash {})", uhs);

        data.xbox_uhs = Some(uhs);
        data.xbl_token = Some(res.token);
        data.xbl_token_expiry = Some(res.not_after);

        Ok(())
    }

    pub async fn xsts_auth(&self) -> anyhow::Result<()> {
        const URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";

        let xbl_token = self.get_xbl_token().await?;

        let mut data = self.data.write().await;

        let xbox_uhs = data.xbox_uhs.as_ref().ok_or(anyhow!("no Xbox user hash"))?;

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

        let uhs = res.display_claims.uhs()?;

        ensure!(*xbox_uhs == uhs, "Xbox user hash mismatch");

        info!("XSTS authorized (user hash {})", uhs);

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

    pub async fn get_xbl_token(&self) -> anyhow::Result<String> {
        if let Some(token) = &self.data.read().await.xbl_token
            && !self.xbl_token_expired().await
        {
            trace!("Xbox Live session is valid, using cached token");

            return Ok(token.clone());
        }

        info!("refreshing Xbox Live token");

        self.xbox_auth().await?;

        let token = self.data.read().await.xbl_token.clone().unwrap();

        Ok(token)
    }

    pub async fn get_xsts_token(&self) -> anyhow::Result<String> {
        if let Some(token) = &self.data.read().await.xsts_token
            && !self.xsts_token_expired().await
        {
            trace!("using cached XSTS token");

            return Ok(token.clone());
        }

        info!("refreshing XSTS token");

        self.xsts_auth().await?;

        let token = self.data.read().await.xsts_token.clone().unwrap();

        Ok(token)
    }
}

#[serde_inline_default]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "PascalCase")]
struct XboxAuthRequest {
    pub properties: Properties,

    #[serde_inline_default("http://auth.xboxlive.com".into())]
    pub relying_party: String,

    #[serde_inline_default("JWT".into())]
    pub token_type: String,
}

impl XboxAuthRequest {
    pub fn new(access_token: String) -> Self {
        Self {
            properties: Properties::new(access_token),
            relying_party: "http://auth.xboxlive.com".into(),
            token_type: "JWT".into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "PascalCase")]
struct XboxAuthResponse {
    pub issue_instant: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub token: String,
    pub display_claims: DisplayClaims,
}

#[serde_inline_default]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "PascalCase")]
struct Properties {
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
struct DisplayClaims {
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
struct Xui {
    pub uhs: String,
}

#[serde_inline_default]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "PascalCase")]
struct XstsAuthRequest {
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

mod xsts {
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
