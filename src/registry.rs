use std::{
    collections::HashMap,
    fmt::{Display, write},
    ops::Deref,
    str::FromStr,
    sync::OnceLock,
    thread,
};

use pubgrub::{DependencyProvider, VersionSet};
use semver::{Version, VersionReq};
use tokio::{
    runtime::{self, Handle, RuntimeFlavor},
    sync::oneshot,
    task::block_in_place,
};
use url::Url;

use crate::{Id, Package, PackageVersion, http::HttpRequest, vanilla::VanillaManage};

pub struct Registry {
    url: Url,
    vanilla: OnceLock<Package>,
}

impl Registry {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            vanilla: OnceLock::new(),
        }
    }

    pub fn location(&self, pack: &Id) -> anyhow::Result<Url> {
        let url = self.url.join(pack.path().to_str().expect("invalid id"))?;
        Ok(url)
    }
}

pub trait RegistryManageImpl {
    fn query_vanilla(&self) -> impl std::future::Future<Output = anyhow::Result<&Package>> + Send;
}

impl<T> RegistryManageImpl for T
where
    T: AsRef<Registry> + VanillaManage + Sync,
{
    async fn query_vanilla(&self) -> anyhow::Result<&Package> {
        let registry = self.as_ref();
        if let Some(pack) = registry.vanilla.get() {
            return Ok(pack);
        }
        let manifest = self.vanilla_manifest().await?;
        let version = manifest
            .versions
            .iter()
            .filter_map(|v| {
                Version::from_str(&v.id).ok().map(|v| PackageVersion {
                    name: format!("Vanilla Minecraft {v}"),
                    desc: "".into(),
                    deps: HashMap::new(),
                })
            })
            .map(|v| (Id::minecraft(), v))
            .collect();
        let pack = Package { version };
        Ok(registry.vanilla.get_or_init(|| pack))
    }
}

pub trait RegistryManage {
    fn query(
        &self,
        pack: &Id,
    ) -> impl std::future::Future<Output = anyhow::Result<&Package>> + Send;
}

impl<T> RegistryManage for T
where
    T: AsRef<Registry> + RegistryManageImpl + HttpRequest + VanillaManage + Sync,
{
    async fn query(&self, pack: &Id) -> anyhow::Result<&Package> {
        // let registry = self.as_ref();

        if *pack == Id::vanilla() {
            return self.query_vanilla().await;
        }

        // let url = registry.location(&pack)?;

        todo!()
    }
}

// #[test]
// fn test() {
//     let url = Url::parse("https://mirrors.ustc.edu.cn/crates.io-index/").unwrap();
//     let url = url.join("./ex/am/example").unwrap();
//     tokio::runtime::Builder::new_current_thread()
//         .enable_all()
//         .build()
//         .unwrap()
//         .block_on(reqwest::get(url))
//         .unwrap();
//     // println!("{url:?}");
// }

pub struct RegistryDependencyProvider<'a, T: RegistryManage>(pub &'a T);

impl<'a, T: RegistryManage> Deref for RegistryDependencyProvider<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a, T> RegistryDependencyProvider<'a, T>
where
    T: RegistryManage + Clone + Send + Sync + 'static,
{
    pub fn sync_query(&self, pack: &Id) -> anyhow::Result<&Package> {
        let x = self.0.clone();
        let pack = pack.clone();
        let y = thread::spawn(move || {
            runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(x.query(&pack))
                .unwrap()
                .clone()
        })
        .join()
        .unwrap();
        todo!()
    }
}

impl<'a, T: RegistryManage> DependencyProvider for RegistryDependencyProvider<'a, T> {
    type P = Id;

    type V = Version;

    type VS = glue::ResolveVersionReq;

    type Priority = Version;

    type M = String;

    type Err = glue::ResolveError;

    fn prioritize(
        &self,
        package: &Self::P,
        range: &Self::VS,
        // TODO(konsti): Are we always refreshing the priorities when `PackageResolutionStatistics`
        // changed for a package?
        package_conflicts_counts: &pubgrub::PackageResolutionStatistics,
    ) -> Self::Priority {
        todo!()
    }

    fn choose_version(
        &self,
        package: &Self::P,
        range: &Self::VS,
    ) -> Result<Option<Self::V>, Self::Err> {
        todo!()
    }

    fn get_dependencies(
        &self,
        package: &Self::P,
        version: &Self::V,
    ) -> Result<pubgrub::Dependencies<Self::P, Self::VS, Self::M>, Self::Err> {
        todo!()
    }
}

// impl DependencyProvider for Registry {
//     type P = String;

//     type V = Version;

//     type VS = glue::ResolveVersionReq;

//     type Priority = Version;

//     type M = String;

//     type Err = glue::ResolveError;

//     fn prioritize(
//         &self,
//         package: &Self::P,
//         range: &Self::VS,
//         // TODO(konsti): Are we always refreshing the priorities when `PackageResolutionStatistics`
//         // changed for a package?
//         package_conflicts_counts: &pubgrub::PackageResolutionStatistics,
//     ) -> Self::Priority {
//         todo!()
//     }

//     fn choose_version(
//         &self,
//         package: &Self::P,
//         range: &Self::VS,
//     ) -> Result<Option<Self::V>, Self::Err> {
//         // range.
//         todo!()
//     }

//     fn get_dependencies(
//         &self,
//         package: &Self::P,
//         version: &Self::V,
//     ) -> Result<pubgrub::Dependencies<Self::P, Self::VS, Self::M>, Self::Err> {
//         todo!()
//     }
// }

mod glue {
    use std::fmt::Display;

    use pubgrub::{Ranges, VersionSet};
    use semver::{Comparator, Op, Version, VersionReq};
    use tokio::runtime::Handle;

    #[derive(Debug)]
    pub struct ResolveError(pub anyhow::Error);

    impl From<anyhow::Error> for ResolveError {
        fn from(value: anyhow::Error) -> Self {
            Self(value)
        }
    }

    impl Display for ResolveError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for ResolveError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.0.source()
        }

        fn description(&self) -> &str {
            "description() is deprecated; use Display"
        }

        fn cause(&self) -> Option<&dyn std::error::Error> {
            self.source()
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ResolveVersionReq(pub Ranges<Version>);

    impl From<VersionReq> for ResolveVersionReq {
        fn from(value: VersionReq) -> Self {
            // Handle::current().blo
            let mut rng = Ranges::full();

            for comp in value.comparators {
                let new = match comp.op {
                    Op::Exact | Op::Wildcard => {
                        Ranges::from_range_bounds(comp.min_version()..=comp.max_version())
                    }
                    Op::Greater => Ranges::strictly_higher_than(comp.max_version()),
                    Op::GreaterEq => Ranges::higher_than(comp.min_version()),
                    Op::Less => Ranges::strictly_lower_than(comp.min_version()),
                    Op::LessEq => Ranges::lower_than(comp.max_version()),
                    _ => todo!("unsupported comparator {:?}", comp.op),
                };
                rng = rng.intersection(&new);
            }

            Self(rng)
        }
    }

    impl Display for ResolveVersionReq {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl VersionSet for ResolveVersionReq {
        type V = Version;

        fn empty() -> Self {
            Self(Ranges::empty())
        }

        fn singleton(v: Self::V) -> Self {
            Self(Ranges::singleton(v))
        }

        fn complement(&self) -> Self {
            Self(self.0.complement())
        }

        fn intersection(&self, other: &Self) -> Self {
            Self(self.0.intersection(&other.0))
        }

        fn contains(&self, v: &Self::V) -> bool {
            self.0.contains(v)
        }
    }

    pub fn resolve_req(req: &VersionReq) -> Ranges<Version> {
        let mut rng = Ranges::full();

        for comp in &req.comparators {
            let new = match comp.op {
                Op::Exact | Op::Wildcard => {
                    Ranges::from_range_bounds(comp.min_version()..=comp.max_version())
                }
                Op::Greater => Ranges::strictly_higher_than(comp.max_version()),
                Op::GreaterEq => Ranges::higher_than(comp.min_version()),
                Op::Less => Ranges::strictly_lower_than(comp.min_version()),
                Op::LessEq => Ranges::lower_than(comp.max_version()),
                _ => todo!("unsupported comparator {:?}", comp.op),
            };
            rng = rng.intersection(&new);
        }

        rng
    }
    // 辅助函数
    trait ComparatorExt {
        fn min_version(&self) -> Version;
        fn max_version(&self) -> Version;
    }

    impl ComparatorExt for Comparator {
        fn min_version(&self) -> Version {
            Version {
                major: self.major,
                minor: self.minor.unwrap_or(0),
                patch: self.patch.unwrap_or(0),
                pre: Default::default(),
                build: Default::default(),
            }
        }

        fn max_version(&self) -> Version {
            Version {
                major: self.major,
                minor: self.minor.unwrap_or(u64::MAX),
                patch: self.patch.unwrap_or(u64::MAX),
                pre: Default::default(),
                build: Default::default(),
            }
        }
    }
}
