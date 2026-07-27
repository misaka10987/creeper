mod meta;
mod prelude;
mod rule;
mod server;

use std::{
    collections::{HashMap, HashSet},
    env::consts::OS,
    iter::once,
    path::PathBuf,
    time::Duration,
};

use crate::{
    Artifact, Checksum, Creeper, Id, Install, builtin::{SyncBuiltinIndex, vanilla_id}, index::{Index, VersionRev, independent_index}, mc::ManifestClient,
};

use anyhow::anyhow;
use creeper_maven_coord::MavenCoord;
use mc_launchermeta::version::library::{Artifact as McArtifact, Library};
use semver::{Version, VersionReq};
use tracing::info;

pub use prelude::*;

pub fn check_class(class: &str) -> bool {
    match class {
        "natives-linux" => OS == "linux",
        "natives-windows" => OS == "windows",
        "natives-macos" | "natives-osx" => OS == "macos",
        c => todo!("unknown classifier {c}"),
    }
}

pub struct VanillaManager {
    manifest: ManifestClient,
}

impl VanillaManager {
    pub fn new(manifest: ManifestClient) -> Self {
        Self { manifest }
    }
}

impl SyncBuiltinIndex for VanillaManager {
    fn package(&self) -> Id {
        vanilla_id()
    }

    async fn sync_index(&self) -> anyhow::Result<Index> {
        let versions = self.manifest.get_version_list().await?;

        let index = independent_index(versions.into_iter().cloned().map(VersionRev::new));

        Ok(index)
    }

    fn cache_expiry(&self) -> std::time::Duration {
        Duration::from_hours(72)
    }
}

impl Creeper {
    pub(crate) async fn vanilla_lib(
        &self,
        lib: impl IntoIterator<Item = Library>,
    ) -> anyhow::Result<HashMap<PathBuf, Artifact>> {
        let arts = filter_lib(lib);

        info!("downloading {} library artifacts", arts.len());

        let lib = arts
            .into_iter()
            .map(|a| {
                let name = MavenCoord::from_path(&a.path)
                    .map(|c| c.to_string())
                    .unwrap_or(a.path.clone());

                (
                    a.path.into(),
                    (name, a.url, Some(a.size), once(Checksum::sha1(a.sha1))),
                )
            })
            .collect();

        let map = self.batch_download(lib).await?;

        Ok(map)
    }

    pub(crate) async fn vanilla_install(&self, version: &Version) -> anyhow::Result<Install> {
        let mc_version = self.vanilla.manifest.get_version(version).await?;

        let install = self.mc_version_install(mc_version.into()).await?;

        let install = Install {
            user: true,
            require_java: mc_java_req(version),
            ..install
        };

        Ok(install)
    }
}

fn filter_lib(lib: impl IntoIterator<Item = Library>) -> Vec<McArtifact> {
    let rule = RuleChecker::default();

    lib.into_iter()
        // apply the rules
        .filter(|x| x.rules.iter().flatten().all(rule.checker()))
        // entries with artifacts to download
        .filter_map(|x| x.downloads)
        // flatten list of artifacts
        .flat_map(|x| {
            x.classifiers
                .into_iter()
                .flatten()
                .filter_map(|(class, art)| check_class(&class).then_some(art))
                .chain(x.artifact)
        })
        // deduplication
        .map(|x| (x.sha1.clone(), x))
        .collect::<HashMap<_, _>>()
        .into_iter()
        .map(|(_k, v)| v)
        .collect()
}

fn java_module_path<'a>(
    args: impl IntoIterator<Item = &'a str>,
) -> anyhow::Result<HashSet<&'a str>> {
    let mut it = args.into_iter().peekable();

    let mut p = HashSet::new();

    while let Some(arg) = it.next() {
        if !(arg == "--module-path" || arg == "-p") {
            continue;
        }

        let value = it
            .peek()
            .ok_or(anyhow!("missing value for java module path"))?;

        let paths = value
            .split("${classpath_separator}")
            .map(|x| x.strip_prefix("${library_directory}/").unwrap_or(x));

        p.extend(paths);
    }

    Ok(p)
}

/// The java version requirement for a specific Minecraft version.
///
/// See [Minecraft Wiki](https://minecraft.wiki/w/Tutorial:Update_Java#Why_update?) for more details.
pub fn mc_java_req(version: &Version) -> VersionReq {
    match version {
        v if v >= &Version::new(26, 1, 0) => ">=25".parse().unwrap(),
        v if v >= &Version::new(1, 20, 5) => ">=21".parse().unwrap(),
        v if v >= &Version::new(1, 18, 0) => ">=17".parse().unwrap(),
        v if v >= &Version::new(1, 17, 0) => ">=16".parse().unwrap(),
        v if v >= &Version::new(1, 12, 0) => ">=1.8.0".parse().unwrap(),
        v if v >= &Version::new(1, 6, 1) => ">=1.6.0".parse().unwrap(),
        _ => ">=1.5.0".parse().unwrap(),
    }
}
