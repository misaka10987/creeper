use std::{collections::HashMap, iter::once, path::PathBuf};

use anyhow::bail;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use tokio::fs::{create_dir_all, read_to_string, remove_file, try_exists, write};
use tracing::debug;

use crate::{Artifact, Creeper, Id, Package, VersionRev, path::creeper_cache_dir};

/// Things installed to the game instance by a package.
#[serde_inline_default]
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

    /// Java agent files and arguments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub java_agent: Vec<JavaAgent>,

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

    /// Whether to disable the minecraft main client `.jar` file, as specified in [`Self::mc_jar`].
    /// Note that setting this value to `false` does nothing,
    /// while setting it to `true` will (irreversibly) disable the minecraft main client `.jar` file in the current game instance.
    ///
    /// The option is present because some packages implement a substitution for the client `.jar` file.
    /// For example, neoforge uses its custom class loader and place the game under java libraries.
    #[serde_inline_default(false)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub disable_mc_jar: bool,

    /// Command line options passed to the Minecraft game program.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mc_flag: Vec<String>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub mc_asset: HashMap<PathBuf, Artifact>,

    /// Minecraft mod files to be added to the `mods` folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mc_mod: Vec<Artifact>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_pack: Vec<Artifact>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shader_pack: Vec<Artifact>,

    #[serde_inline_default(false)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub user: bool,
}

impl Install {
    pub fn merge(self, next: Self) -> Self {
        let mut new = self;
        new.extend(Some(next));
        new
    }

    pub fn simplify(&mut self) {
        self.java_lib_file.retain(|k, _v| {
            !self.java_lib_class.contains_key(k) && !self.java_lib_mod.contains_key(k)
        });
    }
}

impl Extend<Self> for Install {
    fn extend<T: IntoIterator<Item = Self>>(&mut self, iter: T) {
        for next in iter {
            let Self {
                java_lib_class,
                java_lib_mod,
                java_lib_file,
                java_agent,
                java_main_class,
                native,
                java_flag,
                mc_jar,
                disable_mc_jar,
                mc_flag,
                mc_asset,
                mc_mod,
                resource_pack,
                shader_pack,
                user,
            } = next;
            self.java_lib_class.extend(java_lib_class);
            self.java_lib_mod.extend(java_lib_mod);
            self.java_lib_file.extend(java_lib_file);
            self.java_agent.extend(java_agent);
            self.java_main_class = java_main_class.or(self.java_main_class.take());
            self.native.extend(native);
            self.java_flag.extend(java_flag);
            self.mc_jar = mc_jar.or(self.mc_jar.take());
            self.disable_mc_jar = self.disable_mc_jar || disable_mc_jar;
            self.mc_flag.extend(mc_flag);
            self.mc_asset.extend(mc_asset);
            self.mc_mod.extend(mc_mod);
            self.resource_pack.extend(resource_pack);
            self.shader_pack.extend(shader_pack);
            self.user = self.user || user;
        }

        self.simplify();
    }
}

impl Creeper {
    fn install_cache_path(&self, package: &Id, version: &VersionRev) -> anyhow::Result<PathBuf> {
        let path = creeper_cache_dir()?
            .join("install")
            .join(package.indexed_path())
            .join(version.to_string())
            .with_added_extension("json");

        Ok(path)
    }

    // pub(crate) because [`Self::neoforge_install`] needs it to avoid async recursion
    pub(crate) async fn get_install_cache(
        &self,
        package: &Id,
        version: &VersionRev,
    ) -> anyhow::Result<Option<Install>> {
        let cache = self.install_cache_path(package, version)?;

        if !try_exists(&cache).await? {
            return Ok(None);
        }

        let json = read_to_string(&cache).await?;
        let install = serde_json::from_str(&json)?;

        Ok(Some(install))
    }

    pub(crate) async fn set_install_cache(
        &self,
        package: &Id,
        version: &VersionRev,
        value: Option<&Install>,
    ) -> anyhow::Result<()> {
        let cache = self.install_cache_path(package, version)?;

        let install = if let Some(x) = value {
            x
        } else {
            if try_exists(&cache).await? {
                remove_file(&cache).await?;
            }
            return Ok(());
        };

        let json = serde_json::to_string(install)?;
        create_dir_all(cache.parent().unwrap()).await?;
        write(&cache, json).await?;

        Ok(())
    }

    /// Retrieve the installation data for a specific version of package.
    ///
    /// Note that this does not install the dependencies of the package.
    /// Use [`Self::recursive_install`] for that.
    pub async fn install(
        &self,
        package: &Id,
        version: &Version,
        rev: u32,
    ) -> anyhow::Result<Install> {
        if let Some(install) = self
            .get_install_cache(package, &VersionRev::with_rev(version.clone(), rev))
            .await?
        {
            debug!("using cached install {package}@{version}");
            return Ok(install);
        }

        if self.args.offline {
            bail!("{package}@{version}#{rev} is missing from cache");
        }

        let install = if !package.is_regular() {
            self.builtin_install(package, version).await?
        } else {
            let package = self.query_registry(package, version, rev).await?;
            package.install
        };

        self.set_install_cache(
            package,
            &VersionRev::with_rev(version.clone(), rev),
            Some(&install),
        )
        .await?;

        Ok(install)
    }

    /// Install all specified packages in the input.
    /// Automatically merging them with the latter overriding the former.
    pub async fn install_all(
        &self,
        packages: impl IntoIterator<Item = (Id, VersionRev)>,
    ) -> anyhow::Result<Install> {
        let mut install = Install::default();

        for (id, version) in packages {
            let package = self.install(&id, &version.version, version.rev).await?;
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
            let package = self.install(&id, &version.version, version.rev).await?;
            install.extend(once(package));
        }

        install.extend(once(package.install));

        Ok(install)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct JavaAgent {
    pub file: Artifact,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub option: Option<String>,
}
