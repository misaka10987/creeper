use std::path::PathBuf;

use inquire::{Select, Text};
use parse_display::Display;
use serde::{Deserialize, Serialize};
use tracing::warn;
use url::Url;

use crate::{Creeper, path::creeper_config_dir, util::TomlFile};

#[derive(Clone, PartialEq, Eq, Display, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields, rename_all = "kebab-case")]
pub enum User {
    #[display("Offline Player {name}")]
    Offline { name: String },

    #[display("Microsoft Account {account} ({uuid})")]
    Microsoft { account: String, uuid: String },

    #[display("authlib-injector Account {account} ({uuid}) on {server}")]
    AuthlibInjector {
        account: String,
        server: Url,
        uuid: String,
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
            t => todo!("prompt new user type {t}"),
        }
    }

    pub async fn prompt_new_offline_user(&self) -> anyhow::Result<User> {
        let name = Text::new("Player name:").prompt()?;

        let user = User::Offline { name };

        self.user.add(user.clone()).await?;

        Ok(user)
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
            return self.prompt_new_user().await;
        }

        let select = Select::new("Select a user:", users).prompt()?;

        Ok(select)
    }
}
