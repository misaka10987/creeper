use std::{
    fmt::Display,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::bail;
use inquire::{Confirm, Text};
use tokio::fs::{create_dir_all, remove_dir_all, write};
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

    pub async fn parse_or_prompt<T>(&self, s: &str, desc: &str) -> anyhow::Result<T>
    where
        T: FromStr + Send + 'static,
        <T as FromStr>::Err: Display,
    {
        let s = s.to_owned();
        let desc = desc.to_owned();

        if let Ok(val) = s.parse() {
            let confirm = format!("Use {s} as {desc}?");

            let confirm = self
                .inquire()
                .await
                .run(move || Confirm::new(&confirm).prompt())
                .await??;

            if confirm {
                return Ok(val);
            }

            let val = self.prompt_valid(&format!("Input one instead:")).await?;

            return Ok(val);
        }

        let val = self
            .prompt_valid(&format!("{s} is not valid {desc}, input one instead:"))
            .await?;

        Ok(val)
    }

    pub async fn prompt_save(
        &self,
        content: impl AsRef<[u8]>,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let content = content.as_ref();
        let path = path.as_ref();

        let message = format!("Save {} bytes to file?", content.len());

        let confirm = self
            .inquire()
            .await
            .run(move || Confirm::new(&message).with_default(false).prompt())
            .await??;

        if !confirm {
            return Ok(());
        }

        let default = path.display().to_string();

        let path = self
            .inquire()
            .await
            .run(move || {
                Text::new("Enter the path to save to")
                    .with_default(&default)
                    .prompt()
            })
            .await??;

        let path = PathBuf::from(path);

        if let Some(parent) = path.parent() {
            create_dir_all(parent).await?;
        }

        write(&path, content).await?;

        Ok(())
    }
}
