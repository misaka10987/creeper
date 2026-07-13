mod mcsvc;
mod oauth;
mod xbox;

use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::anyhow;
use chrono::Utc;
use oauth2::{
    AccessToken, AuthUrl, ClientId, EndpointNotSet, EndpointSet, RedirectUrl, RefreshToken,
    TokenUrl, basic::BasicClient,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::{
    fs::{create_dir_all, read_to_string, try_exists, write},
    sync::RwLock,
};
use tracing::debug;
use uuid::Uuid;

use crate::path::creeper_data_dir;

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
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
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
    async fn storage_path(&self) -> anyhow::Result<PathBuf> {
        let uuid = self
            .data
            .read()
            .await
            .mc_uuid
            .ok_or(anyhow!("no Minecraft UUID, cannot determine storage path"))?;

        let hash = blake3::hash(uuid.as_bytes()).to_string();

        let path = creeper_data_dir()?
            .join("microsoft")
            .join(hash)
            .with_added_extension("json");

        Ok(path)
    }

    pub async fn load(&self) -> anyhow::Result<()> {
        let path = self.storage_path().await?;

        if !try_exists(&path).await? {
            debug!("no Microsoft session data at {} to load", path.display());
            return Ok(());
        }

        let json = read_to_string(&path).await?;

        let data = serde_json::from_str::<Data>(&json)?;

        *self.data.write().await = data;

        Ok(())
    }

    pub async fn save(&self) -> anyhow::Result<()> {
        let path = self.storage_path().await?;

        let data = self.data.read().await;

        let json = serde_json::to_string(&*data)?;

        create_dir_all(path.parent().unwrap()).await?;

        write(path, json).await?;

        Ok(())
    }

    pub async fn set_uuid(&self, uuid: Uuid) {
        let mut data = self.data.write().await;

        data.mc_uuid = Some(uuid);
    }

    // pub async fn reset_all(&self) {
    //     let uuid = self.get_uuid().await;

    //     let mut data = self.data.write().await;

    //     *data = Data {
    //         mc_uuid: uuid,
    //         ..Default::default()
    //     };
    // }

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
}

fn calc_expiry(expires_in: u64) -> u64 {
    (SystemTime::now() + Duration::from_secs(expires_in))
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
