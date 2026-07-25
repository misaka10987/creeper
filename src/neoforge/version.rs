use std::{fmt::Display, str::FromStr};

use anyhow::{anyhow, ensure};
use semver::Version;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NfVersion {
    ThreeDigit(Version),
    FourDigit(Version, u64),
}

impl NfVersion {
    pub const fn is_3_digit(&self) -> bool {
        matches!(self, Self::ThreeDigit(_))
    }

    pub const fn is_4_digit(&self) -> bool {
        matches!(self, Self::FourDigit(_, _))
    }

    pub const fn is_encode_safe(&self) -> bool {
        match self {
            NfVersion::ThreeDigit(version) => version.major < 26,
            NfVersion::FourDigit(version, _) => version.major >= 26,
        }
    }
}

impl FromStr for NfVersion {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(version) = s.parse::<Version>() {
            ensure!(
                version.major < 26,
                "3-digit neoforge version number after 26: {s}"
            );

            return Ok(Self::ThreeDigit(version));
        }

        let (major, rest) = s
            .split_once(".")
            .ok_or(anyhow!("invalid neoforge version number {s}"))?;

        let major = major.parse()?;

        let rest = rest.parse::<Version>()?;

        let version = Version {
            major,
            minor: rest.major,
            patch: rest.minor,
            pre: rest.pre,
            build: rest.build,
        };

        Ok(Self::FourDigit(version, rest.patch))
    }
}

impl Display for NfVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (version, ext) = match self {
            NfVersion::ThreeDigit(version) => return write!(f, "{version}"),
            NfVersion::FourDigit(v, x) => (v, x),
        };

        let pre = if version.pre.is_empty() {
            "".into()
        } else {
            format!("-{}", version.pre)
        };

        let build = if version.build.is_empty() {
            "".into()
        } else {
            format!("+{}", version.build)
        };

        let version = format!(
            "{}.{}.{}.{}",
            version.major, version.minor, version.patch, ext
        );

        write!(f, "{}{}{}", version, pre, build)
    }
}

impl NfVersion {
    pub fn encode(self) -> anyhow::Result<Version> {
        ensure!(
            self.is_encode_safe(),
            "invalid neoforge version number {self}"
        );

        let (mut version, ext) = match self {
            NfVersion::ThreeDigit(version) => return Ok(version),
            NfVersion::FourDigit(v, x) => (v, x),
        };

        version.patch = (version.patch << 32) | ext;

        Ok(version)
    }

    pub fn decode(version: Version) -> Self {
        if version.major < 26 {
            return Self::ThreeDigit(version);
        }

        let mut version = version;

        let high = version.patch >> 32;
        let low = version.patch & 0xFFFFFFFF;

        version.patch = high;

        Self::FourDigit(version, low)
    }
}
