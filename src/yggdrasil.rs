use std::{path::PathBuf, sync::OnceLock};

use anyhow::{anyhow, bail, ensure};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use tokio::{
    fs::{create_dir_all, read_to_string, write},
    sync::RwLock,
};
use tracing::{debug, info, warn};
use url::Url;
use uuid::Uuid;

use crate::path::creeper_data_dir;

pub struct YggdrasilClient {
    pub server: Url,
    pub username: String,
    http: Client,
    api: OnceLock<Url>,
    access_token: RwLock<Option<String>>,
    client_token: RwLock<Option<String>>,
    selected_profile: RwLock<Option<Profile>>,
    available_profiles: RwLock<Vec<Profile>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Storage {
    pub access_token: Option<String>,
    pub client_token: Option<String>,
    pub selected_profile: Option<Profile>,
    pub available_profiles: Vec<Profile>,
    pub user: Option<Account>,
}

impl YggdrasilClient {
    fn storage_path(&self) -> anyhow::Result<PathBuf> {
        let hash =
            blake3::hash(format!("{} @ {}", self.username, self.server).as_bytes()).to_string();

        let path = creeper_data_dir()?
            .join("yggdrasil")
            .join(hash)
            .with_added_extension("json");

        Ok(path)
    }

    pub fn new(server: Url, username: String, http: Client) -> Self {
        Self {
            server,
            username,
            http,
            api: OnceLock::new(),
            access_token: RwLock::new(None),
            client_token: RwLock::new(None),
            selected_profile: RwLock::new(None),
            available_profiles: RwLock::new(vec![]),
        }
    }

    pub async fn load(&self) -> anyhow::Result<()> {
        let path = self.storage_path()?;

        let json = read_to_string(path).await?;

        let storage = serde_json::from_str::<Storage>(&json)?;

        *self.access_token.write().await = storage.access_token;
        *self.client_token.write().await = storage.client_token;
        *self.selected_profile.write().await = storage.selected_profile;
        *self.available_profiles.write().await = storage.available_profiles;

        Ok(())
    }

    pub async fn save(&self) -> anyhow::Result<()> {
        let path = self.storage_path()?;

        let storage = Storage {
            access_token: self.access_token.read().await.clone(),
            client_token: self.client_token.read().await.clone(),
            selected_profile: self.selected_profile.read().await.clone(),
            available_profiles: self.available_profiles.read().await.clone(),
            user: None,
        };

        let json = serde_json::to_string(&storage)?;

        if let Some(parent) = path.parent() {
            create_dir_all(parent).await?;
        }

        write(path, json).await?;

        Ok(())
    }

    pub async fn is_logged_in(&self) -> bool {
        self.access_token.read().await.is_some()
    }

    pub async fn available_profiles(&self) -> Vec<Profile> {
        self.available_profiles.read().await.clone()
    }

    /// Select a player by UUID for the current session.
    ///
    /// If the player is already selected and the access token is valid, this function does nothing.
    /// Otherwise, it sends a refresh request to the Yggdrasil server to select the player.
    ///
    /// # Note
    ///
    /// To select a player, the player must be known to the client,
    /// which means [`Self::login`] or [`Self::load`] shall be called first.
    pub async fn select(&self, player: &Uuid) -> anyhow::Result<()> {
        let profile = self
            .available_profiles
            .read()
            .await
            .iter()
            .filter(|x| x.id == *player)
            .cloned()
            .collect::<Vec<_>>();

        if profile.len() > 1 {
            bail!("multiple profiles found with the same UUID");
        }

        if self
            .selected_profile
            .read()
            .await
            .as_ref()
            .is_some_and(|x| x.id == *player)
        {
            debug!("already selected profile {player}, checking validity");

            if self.validate().await? {
                debug!("token is valid, no need to select again");
                return Ok(());
            }
        }

        if profile.len() == 0 {
            bail!("can not select player: no profile found with UUID {player}");
        }

        let profile = profile.into_iter().next().unwrap();

        self.refresh(Some(profile)).await?;

        ensure!(
            self.selected_profile
                .read()
                .await
                .as_ref()
                .is_some_and(|x| x.id == *player),
            "server failed to select player {player} after refresh"
        );

        Ok(())
    }

    async fn api(&self) -> anyhow::Result<&Url> {
        if let Some(api) = self.api.get() {
            return Ok(api);
        }

        let res = self.http.get(self.server.clone()).send().await?;

        let mut api = if let Some(ali) = res.headers().get("X-Authlib-Injector-API-Location") {
            let new = self.server.join(ali.to_str()?)?;
            info!("following Yggdrasil ALI redirect: {} -> {new}", self.server);
            new
        } else {
            self.server.clone()
        };

        // to ensure a trailing slash
        if !api.as_str().ends_with("/") {
            api.set_path(&format!("{}/", api.path()));
        }

        if !(api.scheme() == "https") {
            warn!("using non-HTTPS for Yggdrasil API, this is a security vulnerability");
        }

        let api = self.api.get_or_init(|| api);

        Ok(api)
    }

    pub async fn validate(&self) -> anyhow::Result<bool> {
        let access_token = if let Some(token) = self.access_token.read().await.as_ref() {
            token.clone()
        } else {
            debug!("no access token present, skipping validation since automatically invalid");
            return Ok(false);
        };

        let req = ValidateRequest {
            access_token,
            client_token: self.client_token.read().await.clone(),
        };

        let url = self.api().await?.join("authserver/validate")?;

        let res = self.http.post(url).json(&req).send().await?.status();

        Ok(res == StatusCode::NO_CONTENT)
    }

    pub async fn refresh(
        &self,
        selected_profile: Option<Profile>,
    ) -> anyhow::Result<Option<Profile>> {
        let access_token = self
            .access_token
            .read()
            .await
            .as_ref()
            .ok_or(anyhow!("no access token present, cannot refresh"))?
            .clone();

        let req = RefreshRequest {
            access_token,
            client_token: self.client_token.read().await.clone(),
            request_user: true,
            selected_profile,
        };

        let url = self.api().await?.join("authserver/refresh")?;

        let res = self
            .http
            .post(url)
            .json(&req)
            .send()
            .await?
            .json::<RefreshResponse>()
            .await?;

        *self.access_token.write().await = Some(res.access_token);
        *self.client_token.write().await = Some(res.client_token);
        *self.selected_profile.write().await = res.selected_profile.clone();

        Ok(res.selected_profile)
    }

    pub async fn signout(&self, password: &str) -> anyhow::Result<()> {
        let req = SignoutRequest {
            username: self.username.clone(),
            password: password.into(),
        };

        let url = self.api().await?.join("authserver/signout")?;

        let res = self.http.post(url).json(&req).send().await?;

        if res.status() == StatusCode::NO_CONTENT {
            *self.access_token.write().await = None;
            *self.client_token.write().await = None;
            *self.selected_profile.write().await = None;

            Ok(())
        } else {
            bail!("failed to sign out: server returned {}", res.status());
        }
    }

    pub async fn login(&self, password: &str) -> anyhow::Result<()> {
        info!(
            "logging in to {} at Yggdrasil server {}",
            self.username, self.server
        );

        if self.access_token.read().await.is_some() {
            debug!("access token already present, checking validity");

            if self.validate().await? {
                debug!("access token is valid, skipping login");
                return Ok(());
            }

            if let Err(e) = self
                .refresh(self.selected_profile.read().await.clone())
                .await
            {
                debug!("failed to refresh token: {e}, falling back to login");
            } else {
                return Ok(());
            }
        }

        let req = AuthRequest {
            username: self.username.clone(),
            password: password.into(),
            client_token: self.client_token.read().await.clone(),
            request_user: true,
            agent: Default::default(),
        };

        let url = self.api().await?.join("authserver/authenticate")?;

        let res = self
            .http
            .post(url)
            .json(&req)
            .send()
            .await?
            .json::<AuthResponse>()
            .await?;

        *self.access_token.write().await = Some(res.access_token);
        *self.client_token.write().await = Some(res.client_token);
        *self.available_profiles.write().await = res.available_profiles;
        *self.selected_profile.write().await = res.selected_profile.clone();

        Ok(())
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

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AuthResponse {
    pub access_token: String,
    pub client_token: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_profiles: Vec<Profile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_profile: Option<Profile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<Account>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Profile {
    #[serde(with = "uuid::serde::simple")]
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<Property>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Property {
    pub name: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Account {
    #[serde(with = "uuid::serde::simple")]
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<Property>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RefreshRequest {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,
    pub request_user: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_profile: Option<Profile>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RefreshResponse {
    pub access_token: String,
    pub client_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_profile: Option<Profile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<Account>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ValidateRequest {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InvalidateRequest {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SignoutRequest {
    pub username: String,
    pub password: String,
}
