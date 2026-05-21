use std::{collections::HashMap, iter::once, path::PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{Artifact, Creeper, Id, Package};

/// Things installed to the game instance by a package.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Install {
    /// Additional java libraries (classical), prepended to the `--class-path` command line argument when launching the game.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub java_lib_class: HashMap<PathBuf, Artifact>,

    /// Additional java modules (Java 9+), prepended to the `--module-path` command line argument when launching the game.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub java_lib_mod: HashMap<PathBuf, Artifact>,

    /// Additional java library files.
    /// These are placed under the libraries directory, but not automatically added to java CLI arguments.
    ///
    /// This is useful for programs like neoforge which implements custom class loading logic.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub java_lib_file: HashMap<PathBuf, Artifact>,

    /// Java main class override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_main_class: Option<String>,

    /// Native libraries to be added.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub native: HashMap<PathBuf, Artifact>,

    /// Extra java command line options.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub java_flag: Vec<String>,

    /// Minecraft client `.jar` file override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mc_jar: Option<Artifact>,

    /// Command line options passed to the Minecraft game program.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mc_flag: Vec<String>,

    /// Minecraft asset index JSON file override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mc_asset_index: Option<Artifact>,

    /// Minecraft mod files to be added to the `mods` folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mc_mod: Vec<Artifact>,
}

impl Install {
    pub fn merge(self, next: Self) -> Self {
        let mut new = self;
        new.extend(Some(next));
        new
    }
}

impl Extend<Self> for Install {
    fn extend<T: IntoIterator<Item = Self>>(&mut self, iter: T) {
        for next in iter {
            let Self {
                java_lib_class,
                java_lib_mod,
                java_lib_file,
                java_main_class,
                native,
                java_flag,
                mc_jar,
                mc_flag,
                mc_asset_index,
                mc_mod,
            } = next;
            self.java_lib_class.extend(java_lib_class);
            self.java_lib_mod.extend(java_lib_mod);
            self.java_lib_file.extend(java_lib_file);
            self.java_main_class = java_main_class.or(self.java_main_class.take());
            self.native.extend(native);
            self.java_flag.extend(java_flag);
            self.mc_jar = mc_jar.or(self.mc_jar.take());
            self.mc_flag.extend(mc_flag);
            self.mc_asset_index = mc_asset_index.or(self.mc_asset_index.take());
            self.mc_mod.extend(mc_mod);
        }
    }
}

impl Creeper {
    /// Retrieve the installation data for a specific version of package.
    ///
    /// Note that this does not install the dependencies of the package.
    /// Use [`Self::recursive_install`] for that.
    pub async fn install(&self, package: &Id, version: Version) -> anyhow::Result<Install> {
        if !package.is_regular() {
            match package.as_str() {
                "vanilla" => return self.vanilla_install(version).await,
                "neoforge" => return self.neoforge_install(&version).await,
                _ => todo!(),
            }
        }
        let package = self.query_registry(package, &version, 0).await?;
        Ok(package.install)
    }

    /// Install all specified packages in the input.
    /// Automatically merging them with the latter overriding the former.
    pub async fn install_all(
        &self,
        packages: impl IntoIterator<Item = (Id, Version)>,
    ) -> anyhow::Result<Install> {
        let mut install = Install::default();

        for (id, version) in packages {
            let package = self.install(&id, version).await?;
            install.extend(once(package));
        }

        Ok(install)
    }

    /// Recursively retrieve the installation data for the provided package and its dependencies.
    pub async fn recursive_install(&self, package: Package) -> anyhow::Result<Install> {
        let dep = self.resolve(package.node.dep)?;
        let sorted = self.sort_dependency(dep)?;

        let mut install = Install::default();

        for (id, version) in sorted {
            let package = self.install(&id, version).await?;
            install.extend(once(package));
        }

        install.extend(once(package.install));

        Ok(install)
    }
}
