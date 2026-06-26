use std::{
    iter::{once, repeat_n},
    path::PathBuf,
};

use anyhow::bail;
use base64::{Engine, prelude::BASE64_STANDARD};
use inquire::{Select, Text};
use parse_display::Display;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::warn;
use url::Url;
use uuid::Uuid;

use crate::{
    Artifact, Checksum, Creeper, Install, YggdrasilClient, path::creeper_config_dir, util::TomlFile,
};

#[derive(Clone, PartialEq, Eq, Display, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields, rename_all = "kebab-case")]
pub enum User {
    #[display("Offline Player {name}")]
    Offline { name: String },

    #[display("Microsoft Account {account} ({uuid})")]
    Microsoft { account: String, uuid: Uuid },

    #[display("authlib-injector Account {account} ({uuid}) on {server}")]
    AuthlibInjector {
        server: Url,
        account: String,
        uuid: Uuid,
    },
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct UserConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<User>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user: Vec<User>,
}

fn config_path() -> anyhow::Result<PathBuf> {
    let path = creeper_config_dir()?.join("user.toml");
    Ok(path)
}

pub struct UserManager {
    config: TomlFile<UserConfig>,
}

impl UserManager {
    pub fn new() -> Self {
        Self {
            config: TomlFile::new(),
        }
    }

    pub async fn add(&self, user: User) -> anyhow::Result<()> {
        let path = config_path()?;

        let mut config = self.config.read(&path).await?.unwrap_or_default();

        if config.default.as_ref().is_some_and(|x| *x == user) || config.user.contains(&user) {
            warn!("{user} already exists in the config");
            return Ok(());
        }

        config.user.push(user);

        self.config.write(&path, Some(config)).await?;

        Ok(())
    }
}

impl Creeper {
    pub async fn prompt_new_user(&self) -> anyhow::Result<User> {
        let select = Select::new(
            "Type of the new user:",
            vec!["Offline", "Microsoft", "authlib-injector"],
        )
        .prompt()?;

        match select {
            "Offline" => self.prompt_new_offline_user().await,
            "authlib-injector" => self.prompt_new_authlib_injector_user().await,
            t => todo!("prompt new user type {t}"),
        }
    }

    pub async fn prompt_new_offline_user(&self) -> anyhow::Result<User> {
        let name = Text::new("Player name:").prompt()?;

        let user = User::Offline { name };

        self.user.add(user.clone()).await?;

        Ok(user)
    }

    pub async fn prompt_new_authlib_injector_user(&self) -> anyhow::Result<User> {
        let server = Text::new("Yggdrasil server:").prompt()?;

        let account =
            Text::new(&format!("Your account at {server} (usually an email):")).prompt()?;

        let yggdrasil = YggdrasilClient::new(server, account.clone(), self.http.clone())?;

        yggdrasil.load_or_prompt_login().await?;

        let available = yggdrasil.available_profiles().await;

        if available.is_empty() {
            bail!(
                "no availble player for {account} at {}, please create one first",
                yggdrasil.server
            );
        }

        let available = available
            .into_iter()
            .map(|x| User::AuthlibInjector {
                server: yggdrasil.server.clone(),
                account: account.clone(),
                uuid: x.id,
            })
            .collect::<Vec<_>>();

        let select = Select::new("Choose a player:", available).prompt()?;

        let uuid = match &select {
            User::AuthlibInjector { uuid, .. } => uuid,
            _ => unreachable!(),
        };

        yggdrasil.select(uuid).await?;

        yggdrasil.save().await?;

        self.user.add(select.clone()).await?;

        Ok(select)
    }

    pub async fn prompt_select_user(&self) -> anyhow::Result<User> {
        let path = config_path()?;

        let config = self.user.config.read(path).await?.unwrap_or_default();

        let users = config
            .default
            .into_iter()
            .chain(config.user)
            .collect::<Vec<_>>();

        if users.is_empty() {
            eprintln!("No user found in config, please create a new user.");
            return self.prompt_new_user().await;
        }

        let select = Select::new("Select a user:", users).prompt()?;

        Ok(select)
    }

    pub async fn prompt_decide_user(&self) -> anyhow::Result<User> {
        let config = self
            .user
            .config
            .read(config_path()?)
            .await?
            .unwrap_or_default();

        if let Some(user) = config.default {
            return Ok(user);
        }

        self.prompt_select_user().await
    }

    fn user_install_offline(&self, name: String) -> anyhow::Result<Install> {
        let uuid = format!("OfflinePlayer: {name}");

        // to ensure sufficient length
        let uuid = uuid + &repeat_n('\0', 16).collect::<String>();
        let uuid = &uuid[..16];

        let uuid = Uuid::from_slice(uuid.as_bytes())?;

        let install = Install {
            mc_flag: vec![
                "--username".into(),
                name,
                "--uuid".into(),
                uuid.as_simple().to_string(),
                "--accessToken".into(),
                "0".into(),
            ],
            ..Default::default()
        };

        Ok(install)
    }

    async fn user_install_authlib_injector(
        &self,
        server: Url,
        account: String,
        uuid: Uuid,
    ) -> anyhow::Result<Install> {
        let yggdrasil = YggdrasilClient::new(server.to_string(), account, self.http.clone())?;

        yggdrasil.load_or_prompt_login().await?;

        let name = yggdrasil.select(&uuid).await?.name;

        let token = yggdrasil.get_token().await?;

        let prefetch = yggdrasil.prefetch().await?;
        let prefetch = BASE64_STANDARD.encode(serde_json::to_string(&prefetch)?);

        let prefetch_arg = format!("-Dauthlibinjector.yggdrasil.prefetched={prefetch}");

        let api = yggdrasil.api().await?;

        yggdrasil.save().await?;

        let jar = self.latest_authlib_injector().await?;

        let install = Install {
            java_agent: vec![(jar, Some(api.to_string()))],
            java_flag: vec![prefetch_arg],
            mc_flag: vec![
                "--username".into(),
                name,
                "--uuid".into(),
                uuid.as_simple().to_string(),
                "--accessToken".into(),
                token,
            ],
            ..Default::default()
        };

        Ok(install)
    }

    async fn latest_authlib_injector(&self) -> anyhow::Result<Artifact> {
        const URL: &str = "https://authlib-injector.yushi.moe/artifact/latest.json";

        let version = self
            .http
            .get(URL)
            .send()
            .await?
            .json::<AuthlibInjectorVersion>()
            .await?;

        let name = format!("authlib-injector-{}.jar", version.version);

        let art = self
            .download(
                name,
                version.download_url.to_string(),
                None,
                once(Checksum::sha256(version.checksums.sha256)),
            )
            .await?;

        Ok(art)
    }

    pub async fn user_install(&self) -> anyhow::Result<Install> {
        let user = self.prompt_decide_user().await?;

        let install = match user {
            User::Offline { name } => self.user_install_offline(name)?,
            User::AuthlibInjector {
                server,
                account,
                uuid,
            } => {
                self.user_install_authlib_injector(server, account, uuid)
                    .await?
            }
            _ => todo!(),
        };

        Ok(install)
    }
}

mod authlib_injector {
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Serialize, Deserialize)]
    pub struct Checksums {
        pub sha256: String,
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct AuthlibInjectorVersion {
    pub build_number: u64,
    pub version: Version,
    pub release_time: String,
    pub download_url: Url,
    pub checksums: authlib_injector::Checksums,
}

// #[test]
// fn test() {
//     let json = r#"
//     {
//   "build_number": 55,
//   "version": "1.2.7",
//   "release_time": "2025-12-16T16:01:27Z",
//   "download_url": "https://authlib-injector.yushi.moe/artifact/55/authlib-injector-1.2.7.jar",
//   "checksums": {
//     "sha256": "eaf14bc5acffc7d885bd5bd5942b99f36d6299302beae356b2fc5807fe42652b"
//   }
// }
//     "#;

//     let version = serde_json::from_str::<AuthlibInjectorVersion>(json).unwrap();
// }
