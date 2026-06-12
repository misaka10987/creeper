use std::{
    fmt::Display,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{anyhow, bail, ensure};
use semver::Version;
use tracing::warn;

/// Check whether the given string is a valid domain name for a java package.
pub fn is_valid_java_package(name: &str) -> bool {
    let name = name.replace("-", "_");

    for piece in name.split(".") {
        let mut chars = piece.chars();

        if chars.next().is_none_or(|c| !c.is_ascii_lowercase()) {
            return false;
        }

        if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return false;
        }
    }

    true
}

/// Check whether the given string is a valid [maven artifact identifier](https://maven.apache.org/guides/mini/guide-naming-conventions.html#artifact-identifier), i.e. consists only of lowercase ascii letters, digits and hyphens.
pub fn is_valid_maven_artifact(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// fn normalize_version(version: &str) -> String {
//     let (version, suffix) = version
//         .split_once("-")
//         .or(version.split_once("+"))
//         .unwrap_or((version, ""));

//     match version.matches(".").count() {
//         0 => format!("{}.0.0{}", version, suffix),
//         1 => format!("{}.0{}", version, suffix),
//         _ => format!("{}{}", version, suffix),
//     }
// }

/// A maven coordinate defined as `<groupId>:<artifactId>:<version>`.
#[derive(Clone, PartialEq, Eq)]
pub struct MavenCoord {
    /// The group id.
    ///
    /// # Note
    ///
    /// The bahavior is undefined unless this is a valid java package name, i.e. [`is_valid_java_package`].
    pub group: String,

    /// The artifact id.
    pub artifact: String,

    /// The version.
    ///
    /// This is at most times, but not guaranteed to be, a semver.
    pub version: String,

    /// The path extension, defaults to `"jar"` if not specified.
    pub extension: Option<String>,

    /// The classifier.
    pub classifier: Option<String>,
}

#[cfg(test)]
#[test]
fn test() {
    const COORD: &str = "net.neoforged:neoform:1.21.1-20240808.144430:mappings-merged@txt";
    const PATH: &str = "net/neoforged/neoform/1.21.1-20240808.144430/neoform-1.21.1-20240808.144430-mappings-merged.txt";

    let x = COORD.parse::<MavenCoord>().unwrap();
    let y = MavenCoord::from_path(PATH).unwrap();

    assert!(x == y);
    assert!(x.path() == *PATH);
    assert!(y.to_string() == COORD);
}

impl MavenCoord {
    pub fn new(
        group: String,
        artifact: String,
        version: String,
        classifier: Option<String>,
        extension: Option<String>,
    ) -> anyhow::Result<Self> {
        if !is_valid_java_package(&group) {
            bail!("invalid group {group} in maven coordinate");
        }

        if !is_valid_maven_artifact(&artifact) {
            // does only warn because neoforge uses CamelCase in some artifact names
            warn!("invalid artifact {artifact} in maven coordinate");
        }

        if !version.parse::<Version>().is_ok() {
            warn!(
                "version {version} in maven coordinate is not valid semver, this is not recommended"
            );
        }

        Ok(Self {
            group,
            artifact,
            version,
            classifier,
            extension,
        })
    }

    pub fn path(&self) -> PathBuf {
        let classifier = if let Some(s) = &self.classifier {
            &format!("-{s}")
        } else {
            ""
        };

        let extension = if let Some(s) = &self.extension {
            &format!(".{s}")
        } else {
            ".jar"
        };

        let name = format!("{}-{}{classifier}{extension}", self.artifact, self.version);

        self.group
            .split(".")
            .fold(PathBuf::new(), |acc, x| acc.join(x))
            .join(&self.artifact)
            .join(self.version.to_string())
            .join(name)
    }

    pub fn from_path(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let mut path = path.as_ref().to_path_buf();

        let extension = if let Some(s) = path.extension() {
            let name = s
                .to_str()
                .ok_or(anyhow!("invalid maven path extension {}", s.display()))?;

            if name == "jar" {
                None
            } else {
                Some(name.into())
            }
        } else {
            Some("".into())
        };

        path.set_extension("");

        let mut components = vec![];

        for c in path.components() {
            let name = c.as_os_str();

            let name = name
                .to_str()
                .ok_or(anyhow!("invalid maven path component {}", name.display()))?;
            components.push(name);
        }

        ensure!(
            components.len() >= 4,
            "invalid maven path {}, expected at least 4 components",
            path.display()
        );

        let last = components[components.len() - 1];
        let version = components[components.len() - 2];
        let artifact = components[components.len() - 3];

        let classifier = last
            .strip_prefix(&format!("{artifact}-{version}"))
            .ok_or(anyhow!("invalid maven path {}", path.display()))?;

        let classifier = if classifier.is_empty() {
            None
        } else {
            Some(
                classifier
                    .strip_prefix("-")
                    .ok_or(anyhow!("invalid classifier {classifier} in maven path"))?,
            )
        };

        let group = components[..components.len() - 3].join(".");

        Ok(Self::new(
            group,
            artifact.into(),
            version.into(),
            classifier.map(|s| s.into()),
            extension,
        )?)
    }
}

impl Display for MavenCoord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let classifier = if let Some(s) = &self.classifier {
            &format!(":{s}")
        } else {
            ""
        };

        let extension = if let Some(s) = &self.extension {
            &format!("@{s}")
        } else {
            ""
        };

        write!(
            f,
            "{}:{}:{}{classifier}{extension}",
            self.group, self.artifact, self.version
        )
    }
}

impl FromStr for MavenCoord {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let pieces = s.split("@").collect::<Vec<_>>();

        let (main, extension) = match pieces.len() {
            0 => unreachable!(),
            1 => (pieces[0], None),
            2 => (pieces[0], Some(pieces[1])),
            _ => bail!(
                "invalid maven coordinate {s}, expected <group>:<artifact>:<version>[:<classifier>][@<extension>]"
            ),
        };

        let pieces = main.split(":").collect::<Vec<_>>();

        let (group, artifact, version, classifier) = match pieces.len() {
            0 => unreachable!(),
            3 => (pieces[0], pieces[1], pieces[2], None),
            4 => (pieces[0], pieces[1], pieces[2], Some(pieces[3])),
            _ => bail!(
                "invalid maven coordinate {s}, expected <group>:<artifact>:<version>[:<classifier>][@<extension>]"
            ),
        };

        Ok(Self::new(
            group.into(),
            artifact.into(),
            version.into(),
            classifier.map(|s| s.into()),
            extension.map(|s| s.into()),
        )?)
    }
}
