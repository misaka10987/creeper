mod meta;
mod prelude;
mod rule;
mod server;

use std::{
    collections::{HashMap, HashSet},
    env::consts::OS,
    iter::once,
    path::PathBuf,
    sync::OnceLock,
    time::Duration,
};

use crate::{
    Artifact, Checksum, Creeper, Id, Install,
    builtin::SyncBuiltinIndex,
    index::{Index, VersionRev, independent_index},
};

use anyhow::anyhow;
use creeper_maven_coord::MavenCoord;
use mc_launchermeta::{
    VERSION_MANIFEST_URL,
    version::{
        Version as McVersion,
        library::{Artifact as McArtifact, Library},
    },
    version_manifest::Manifest,
};
use reqwest::Client;
use semver::Version;
use tokio::sync::RwLock;
use tracing::{debug, info, trace};

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
    http: Client,
    manifest: OnceLock<Manifest>,
    version: RwLock<HashMap<Version, McVersion>>,
}

impl VanillaManager {
    pub fn new(http: Client) -> Self {
        Self {
            http,
            manifest: OnceLock::new(),
            version: RwLock::new(HashMap::new()),
        }
    }
}

impl SyncBuiltinIndex for VanillaManager {
    fn package(&self) -> Id {
        Id::vanilla()
    }

    async fn sync_index(&self) -> anyhow::Result<Index> {
        info!("updating vanilla metadata");

        let req = self.http.get(VERSION_MANIFEST_URL).build()?;
        let res = self.http.execute(req).await?;

        let manifest = res.json::<Manifest>().await?;

        let mut versions = vec![];

        let count = manifest.versions.len();

        for version in manifest.versions {
            if let Some(version) = version.id.parse().ok() {
                versions.push(version);
            } else {
                trace!("ignoring invalid vanilla version {}", version.id);
            }
        }

        debug!(
            "retrieved {count} vanilla versions, of which {} valid",
            versions.len()
        );

        let index = independent_index(versions.into_iter().map(VersionRev::new));

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

    pub async fn vanilla_manifest(&self) -> anyhow::Result<&Manifest> {
        if let Some(manifest) = self.vanilla.manifest.get() {
            return Ok(manifest);
        }
        info!("synchronizing minecraft version manifest");

        let req = self.http.get(VERSION_MANIFEST_URL).build()?;
        let res = self.http.execute(req).await?;

        let manifest = res.json().await?;

        Ok(self.vanilla.manifest.get_or_init(|| manifest))
    }

    pub async fn vanilla_version(&self, version: Version) -> anyhow::Result<McVersion> {
        if let Some(mc_version) = self.vanilla.version.read().await.get(&version) {
            return Ok(mc_version.clone());
        }
        info!("synchronizing minecraft {version} version metadata");
        let manifest = self.vanilla_manifest().await?;
        let url = manifest
            .get_version(&version.to_string())
            .ok_or(anyhow!("minecraft version {version} not found in manifest"))?
            .url
            .to_owned();

        let req = self.http.get(url).build()?;
        let res = self.http.execute(req).await?;
        let mc_version = res.json::<McVersion>().await?;

        self.vanilla
            .version
            .write()
            .await
            .insert(version, mc_version.clone());
        Ok(mc_version)
    }

    pub(crate) async fn vanilla_install(&self, version: &Version) -> anyhow::Result<Install> {
        let mc_version = self.vanilla_version(version.clone()).await?;

        let install = self.mc_version_install(mc_version.into()).await?;

        let install = Install {
            user: true,
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
