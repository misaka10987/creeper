use std::{
    fmt::Display,
    time::{Duration, SystemTime},
};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, trace};
use uuid::Uuid;

use crate::ms::{MicrosoftClient, calc_expiry};

impl MicrosoftClient {
    pub async fn mc_jwt_expired(&self) -> bool {
        let data = self.data.read().await;

        let expiry = match data.mc_jwt_expiry {
            Some(time) => time,
            None => return true,
        };

        let expiry = SystemTime::UNIX_EPOCH + Duration::from_secs(expiry);

        SystemTime::now() + Duration::from_secs(15 * 60) >= expiry
    }

    pub async fn get_mc_jwt(&self) -> anyhow::Result<String> {
        if let Some(token) = &self.data.read().await.mc_jwt
            && !self.mc_jwt_expired().await
        {
            trace!("Minecraft session is valid, using cached token");

            return Ok(token.clone());
        }

        info!("refreshing Minecraft JWT");

        self.mc_login().await?;

        let token = self.data.read().await.mc_jwt.clone().unwrap();

        Ok(token)
    }

    pub async fn mc_login(&self) -> anyhow::Result<()> {
        const URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";

        let xsts_token = self.get_xsts_token().await?;

        let mut data = self.data.write().await;

        let uhs = data.xbox_uhs.as_ref().ok_or(anyhow!("no Xbox user hash"))?;

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

        info!("logged in to Minecraft services (user id {})", res.username);

        data.mc_jwt = Some(res.access_token);
        data.mc_jwt_expiry = Some(calc_expiry(res.expires_in));

        Ok(())
    }

    pub async fn owns_minecraft(&self) -> anyhow::Result<bool> {
        const URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";

        let mc_jwt = self.get_mc_jwt().await?;

        let res = self
            .http
            .get(URL)
            .bearer_auth(mc_jwt)
            .send()
            .await?
            .error_for_status()?
            .json::<McStoreResponse>()
            .await?;

        let own = !res.items.is_empty();

        if own {
            debug!("user owns a Minecraft purchase");
        } else {
            debug!("user does not own a Minecraft purchase");
        }

        Ok(own)
    }

    pub async fn sync_mc_user(&self) -> anyhow::Result<()> {
        const URL: &str = "https://api.minecraftservices.com/minecraft/profile";

        let mc_jwt = self.get_mc_jwt().await?;

        let mut data = self.data.write().await;

        let res = self
            .http
            .get(URL)
            .bearer_auth(mc_jwt)
            .send()
            .await?
            .error_for_status()?
            .json::<McProfileResponse>()
            .await?;

        data.mc_uuid = Some(res.id);
        data.mc_name = Some(res.name);

        Ok(())
    }

    pub async fn get_mc_uuid(&self) -> anyhow::Result<Uuid> {
        if let Some(uuid) = self.data.read().await.mc_uuid {
            return Ok(uuid);
        }

        self.sync_mc_user().await?;

        let uuid = self.data.read().await.mc_uuid.unwrap();

        Ok(uuid)
    }

    pub async fn get_mc_name(&self) -> anyhow::Result<String> {
        if let Some(name) = self.data.read().await.mc_name.clone() {
            return Ok(name);
        }

        self.sync_mc_user().await?;

        let name = self.data.read().await.mc_name.clone().unwrap();

        Ok(name)
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
    pub username: Uuid,
    pub roles: Vec<String>,
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub metadata: serde_json::Value,
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
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    pub struct Skin {
        pub id: Uuid,

        pub state: String,

        pub url: Url,

        pub texture_key: String,

        pub variant: String,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub alias: Option<String>,
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    pub struct Cape {
        pub id: Uuid,

        pub state: String,

        pub url: Url,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub alias: Option<String>,
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McProfileResponse {
    #[serde(with = "uuid::serde::simple")]
    pub id: Uuid,

    pub name: String,

    pub skins: Vec<mc::Skin>,

    pub capes: Vec<mc::Cape>,

    pub profile_actions: serde_json::Value,
}
