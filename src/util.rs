use std::{
    collections::HashMap,
    fmt::Display,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{anyhow, bail, ensure};
use async_zip::base::read::seek::ZipFileReader;
use serde::{Deserialize, Serialize};
use tokio::{
    fs::{File, copy, create_dir_all, metadata, remove_file, rename, set_permissions},
    io::BufReader,
};

pub async fn mv(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> anyhow::Result<()> {
    if let Some(parent) = dst.as_ref().parent() {
        create_dir_all(parent).await?;
    }
    File::create(&dst).await?;

    let rename = rename(&src, &dst).await;
    match rename {
        Ok(_) => return Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {}
        e => e?,
    }
    copy(&src, &dst).await?;
    remove_file(&src).await?;
    Ok(())
}

pub async fn set_readonly(path: impl AsRef<Path>) -> anyhow::Result<()> {
    let path = path.as_ref();

    let metadata = metadata(path).await?;

    let mut perm = metadata.permissions();
    perm.set_readonly(true);

    set_permissions(path, perm).await?;

    Ok(())
}

/// Extract a text file from a zip archive `zip_file` at the path `path`.
///
/// # Panics
///
/// The function panics unless `path` is valid UTF-8.
pub async fn extract_zip(
    zip_file: impl AsRef<Path>,
    path: impl AsRef<Path>,
) -> anyhow::Result<String> {
    let zip_file = zip_file.as_ref();
    let path = path.as_ref();

    let zip = File::open(&zip_file).await?;
    let read = BufReader::new(zip);

    let mut zip = ZipFileReader::with_tokio(read).await?;

    let idx = zip
        .file()
        .entries()
        .iter()
        .position(|e| {
            e.filename()
                .as_str()
                .is_ok_and(|s| s == path.to_str().unwrap())
        })
        .ok_or(anyhow!(
            "{} not found in {}",
            path.display(),
            zip_file.display()
        ))?;

    let mut read = zip.reader_with_entry(idx).await?;

    let mut buf = String::new();
    read.read_to_string_checked(&mut buf).await?;

    Ok(buf)
}

/// Parse the first section of an RFC 822-like format.
///
/// # Note
///
/// TODO: this function does not yet guarantee complete support for the RFC 822 and there may exist behavioral difference in edge cases.
pub fn rfc822_first_section(s: &str) -> anyhow::Result<HashMap<&str, &str>> {
    let mut map = HashMap::new();

    let lines = s.lines().take_while(|l| !l.is_empty());

    for line in lines {
        let (key, value) = line.split_once(": ").ok_or(anyhow!("invalid line"))?;
        map.insert(key, value);
    }

    Ok(map)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct JarManifest {
    pub manifest_version: String,
    pub implementation_version: Option<String>,
    pub main_class: Option<String>,
}

impl FromStr for JarManifest {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let map = rfc822_first_section(s)?;

        let manifest_version = map
            .get("Manifest-Version")
            .ok_or(anyhow!("missing field Manifest-Version"))?
            .to_string();
        let implementation_version = map.get("Implementation-Version").map(|s| s.to_string());
        let main_class = map.get("Main-Class").map(|s| s.to_string());

        Ok(Self {
            manifest_version,
            implementation_version,
            main_class,
        })
    }
}

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
}

impl MavenCoord {
    pub fn path(&self) -> PathBuf {
        self.group
            .split(".")
            .fold(PathBuf::new(), |acc, x| acc.join(x))
            .join(&self.artifact)
            .join(self.version.to_string())
            .join(format!("{}-{}", self.artifact, self.version))
    }
}

impl Display for MavenCoord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.group, self.artifact, self.version)
    }
}

impl FromStr for MavenCoord {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let pieces = s.split(":").collect::<Vec<_>>();

        ensure!(
            pieces.len() == 3,
            "invalid maven id {s}, expected <group>:<artifact>:<version>"
        );

        let (group, artifact, version) = (pieces[0], pieces[1], pieces[2]);

        if !is_valid_java_package(group) {
            bail!("invalid group id {group}");
        }

        if !artifact
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            bail!("invalid artifact id {artifact}");
        }

        // let version = normalize_version(version).parse()?;

        Ok(Self {
            group: group.into(),
            artifact: artifact.into(),
            version: version.into(),
        })
    }
}
