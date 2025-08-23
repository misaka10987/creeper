use std::{
    fmt::Display,
    fs::File,
    io::{BufReader, Read},
    path::Path,
    str::FromStr,
};

use anyhow::anyhow;
use const_hex::ToHexExt;
use ring::digest::{Algorithm, Context, SHA1_FOR_LEGACY_USE_ONLY, SHA256};
use tokio::task::spawn_blocking;

pub async fn blake3(file: impl AsRef<Path>) -> anyhow::Result<String> {
    fn calc(file: impl AsRef<Path>) -> anyhow::Result<String> {
        let reader = File::open(file)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update_reader(reader)?;
        let hash = hasher.finalize().to_hex().to_string();
        Ok(hash)
    }
    let file = file.as_ref().to_owned();
    spawn_blocking(|| calc(file)).await?
}

pub async fn sha1(file: impl AsRef<Path>) -> anyhow::Result<String> {
    let file = file.as_ref().to_owned();
    spawn_blocking(|| ring(file, &SHA1_FOR_LEGACY_USE_ONLY)).await?
}

pub async fn sha256(file: impl AsRef<Path>) -> anyhow::Result<String> {
    let file = file.as_ref().to_owned();
    spawn_blocking(|| ring(file, &SHA256)).await?
}

fn ring(file: impl AsRef<Path>, algorithm: &'static Algorithm) -> anyhow::Result<String> {
    let mut reader = BufReader::new(File::open(file)?);
    let mut ctx = Context::new(algorithm);
    let mut buf = [0u8; 4096];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        ctx.update(&buf[..n]);
    }
    let digest = ctx.finish();
    Ok(digest.encode_hex())
}

#[derive(Clone, Hash)]
pub struct Checksum {
    pub function: HashFunc,
    pub hex_hash: String,
}

impl Checksum {
    pub fn blake3(hex_hash: String) -> Self {
        Self {
            function: HashFunc::Blake3,
            hex_hash,
        }
    }

    pub fn sha1(hex_hash: String) -> Self {
        Self {
            function: HashFunc::Sha1,
            hex_hash,
        }
    }

    pub fn sha256(hex_hash: String) -> Self {
        Self {
            function: HashFunc::Sha256,
            hex_hash,
        }
    }

    pub async fn check(&self, file: impl AsRef<Path>) -> anyhow::Result<bool> {
        let hash = self.function.calc(file).await?;
        Ok(self.hex_hash == hash)
    }
}

impl Display for Checksum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}={}", self.function, self.hex_hash)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum HashFunc {
    Blake3,
    Sha1,
    Sha256,
}

impl HashFunc {
    pub async fn calc(&self, file: impl AsRef<Path>) -> anyhow::Result<String> {
        let file = file.as_ref().to_owned();
        match self {
            HashFunc::Blake3 => blake3(file).await,
            HashFunc::Sha1 => sha1(file).await,
            HashFunc::Sha256 => sha256(file).await,
        }
    }
}

impl Display for HashFunc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            HashFunc::Blake3 => "blake3",
            HashFunc::Sha1 => "sha1",
            HashFunc::Sha256 => "sha256",
        };
        write!(f, "{name}")
    }
}

impl FromStr for HashFunc {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "blake3" => Ok(Self::Blake3),
            "sha1" => Ok(Self::Sha1),
            "sha256" => Ok(Self::Sha256),
            _ => Err(anyhow!("unknown hash function: {s}")),
        }
    }
}
