use std::{fmt::Display, path::Path, str::FromStr};

use anyhow::bail;
use inquire::{Confirm, Text};
use tokio::fs::remove_dir_all;
use tracing::info;

use crate::{Creeper, inquire::parse_validator};

impl Creeper {
    /// Prompt the user to confirm the removal of a file or directory, and remove it if confirmed.
    pub async fn prompt_remove(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = path.as_ref();

        let msg = format!("Remove {}?", path.display());

        let confirm = self
            .inquire()
            .await
            .run(move || Confirm::new(&msg).prompt())
            .await??;

        if !confirm {
            bail!("aborted by user")
        }

        info!("removing {}", path.display());

        remove_dir_all(path).await?;

        Ok(())
    }

    pub async fn prompt_valid<T>(&self, msg: &str) -> anyhow::Result<T>
    where
        T: FromStr + Send + 'static,
        <T as FromStr>::Err: Display,
    {
        let msg = msg.to_string();

        let value = self
            .inquire()
            .await
            .run(move || {
                Text::new(&msg)
                    .with_validator(parse_validator::<T>())
                    .prompt()
            })
            .await??
            .parse()
            .map_err(|_| unreachable!())
            .unwrap();

        Ok(value)
    }

    pub async fn prompt_correct_license(&self, exp: &str) -> anyhow::Result<spdx::Expression> {
        match exp.parse() {
            Ok(x) => Ok(x),
            Err(_) if let Ok(x) = format!("LicenseRef-{exp}").parse() => Ok(x),
            Err(_) => {
                self.prompt_valid(&format!(
                    "{exp} is not valid SPDX license expression, input one instead:"
                ))
                .await
            }
        }
    }
}
