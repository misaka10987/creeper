use std::{fmt::Display, str::FromStr};

use anyhow::{bail, ensure};
use semver::VersionReq;
use serde_with::{DeserializeFromStr, SerializeDisplay};

#[derive(Clone, PartialEq, Eq, SerializeDisplay, DeserializeFromStr)]
#[cfg_attr(test, derive(Debug))]
pub enum MavenVersionRange {
    /// Exactly equal to.
    Exact(String),

    /// Less than or equal to.
    LE(String),

    /// Strictly less than.
    LT(String),

    /// Greater than or equal to.
    GE(String),

    /// Strictly greater than.
    GT(String),

    /// Not equal to.
    NE(String),

    /// Open interval.
    Open(String, String),

    /// Closed interval.
    Closed(String, String),

    /// Disjoint union of multiple sets.
    Multiple(Vec<MavenVersionRange>),

    /// Intervals of the form $[a, b)$.
    LowerLimit(String, String),
}

impl TryFrom<MavenVersionRange> for VersionReq {
    type Error = anyhow::Error;

    fn try_from(value: MavenVersionRange) -> Result<Self, Self::Error> {
        match value {
            MavenVersionRange::Exact(v) => Ok(format!("={v}").parse()?),
            MavenVersionRange::LE(v) => Ok(format!("<={v}").parse()?),
            MavenVersionRange::LT(v) => Ok(format!("<{v}").parse()?),
            MavenVersionRange::GE(v) => Ok(format!(">={v}").parse()?),
            MavenVersionRange::GT(v) => Ok(format!(">{v}").parse()?),
            MavenVersionRange::NE(_) => bail!("does not support not-equal-to operator"),
            MavenVersionRange::Open(l, r) => Ok(format!(">{l}, <{r}").parse()?),
            MavenVersionRange::Closed(l, r) => Ok(format!(">={l}, <={r}").parse()?),
            MavenVersionRange::Multiple(_) => bail!("does not support union of intervals"),
            MavenVersionRange::LowerLimit(l, r) => Ok(format!(">={l}, <{r}").parse()?),
        }
    }
}

impl Display for MavenVersionRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MavenVersionRange::Exact(v) => write!(f, "[{v}]"),
            MavenVersionRange::LE(v) => write!(f, "(,{v}]"),
            MavenVersionRange::LT(v) => write!(f, "(,{v})"),
            MavenVersionRange::GE(v) => write!(f, "[{v},)"),
            MavenVersionRange::GT(v) => write!(f, "({v},)"),
            MavenVersionRange::NE(v) => write!(f, "(,{v}),({v},)"),
            MavenVersionRange::Open(start, end) => write!(f, "({start},{end})"),
            MavenVersionRange::Closed(start, end) => write!(f, "[{start},{end}]"),
            MavenVersionRange::Multiple(segments) => {
                write!(
                    f,
                    "{}",
                    segments
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
            MavenVersionRange::LowerLimit(l, r) => write!(f, "[{l},{r})"),
        }
    }
}

impl FromStr for MavenVersionRange {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.contains("(") && !s.contains("[") {
            return Ok(Self::GE(s.into()));
        }

        let left = s
            .char_indices()
            .filter(|(_, c)| ['(', '['].contains(c))
            .collect::<Vec<_>>();
        let right = s
            .char_indices()
            .filter(|(_, c)| [')', ']'].contains(c))
            .collect::<Vec<_>>();

        ensure!(left.len() == right.len(), "bracket mismatch: {s}");

        let segments = left.into_iter().zip(right).collect::<Vec<_>>();

        let mut it = segments.iter().peekable();

        while let Some(((i_l, _), (i_r, _))) = it.next() {
            ensure!(i_l < i_r, "bracket mismatch: {s}");
            if let Some(((i_l_next, _), _)) = it.peek() {
                ensure!(i_r < i_l_next, "bracket mismatch: {s}");
                ensure!(
                    *i_l_next == *i_r + 2 && s.chars().nth(i_r + 1).unwrap() == ',',
                    "not comma separated: {s}"
                );
            }
        }

        let segments = segments
            .iter()
            .map(|((i_l, c_l), (i_r, c_r))| (&s[*i_l..=*i_r], c_l, c_r))
            .collect::<Vec<_>>();

        if segments.len() > 1 {
            let mut range = vec![];

            for (s, _, _) in segments {
                let segment = Self::from_str(s)?;
                range.push(segment);
            }

            if range.len() == 2
                && let Self::LT(x) = &range[0]
                && let Self::GT(y) = &range[1]
                && x == y
            {
                return Ok(Self::NE(x.clone()));
            }

            return Ok(Self::Multiple(range));
        }

        let (v, l, r) = segments[0];

        let split = v[1..v.len() - 1] // strip the brackets
            .split(",")
            .collect::<Vec<_>>();

        ensure!(
            split.len() <= 2,
            "invalid interval {s}, expected no more than two comma(s)"
        );

        let mut it = split.into_iter();
        let start = it.next().unwrap();
        let end = it.next();

        match (start, end, l, r) {
            ("", Some(end), '(', ']') => Ok(Self::LE(end.into())),
            ("", Some(end), '(', ')') => Ok(Self::LT(end.into())),
            ("", Some(_), _, _) => bail!("invalid interval {v}"),

            (start, None, '[', ']') => Ok(Self::Exact(start.into())),
            (_, None, _, _) => bail!("invalid interval {v}"),

            (start, Some(""), '[', ')') => Ok(Self::GE(start.into())),
            (start, Some(""), '(', ')') => Ok(Self::GT(start.into())),
            (_, Some(""), _, _) => bail!("invalid interval {v}"),

            (start, Some(end), '(', ')') => Ok(Self::Open(start.into(), end.into())),
            (start, Some(end), '[', ']') => Ok(Self::Closed(start.into(), end.into())),
            (start, Some(end), '[', ')') => Ok(Self::LowerLimit(start.into(), end.into())),
            (_, Some(_), _, _) => bail!("invalid interval {v}"),
        }
    }
}

#[cfg(test)]
mod test {
    use semver::VersionReq;

    use crate::MavenVersionRange;

    #[test]
    fn maven_version_range() {
        let mut data = vec![];

        data.push(("1.0", MavenVersionRange::GE("1.0".into())));
        data.push(("(,1.0]", MavenVersionRange::LE("1.0".into())));
        data.push(("(,1.0)", MavenVersionRange::LT("1.0".into())));
        data.push(("[1.0]", MavenVersionRange::Exact("1.0".into())));
        data.push(("[1.0,)", MavenVersionRange::GE("1.0".into())));
        data.push(("(1.0,)", MavenVersionRange::GT("1.0".into())));
        data.push((
            "(1.0,2.0)",
            MavenVersionRange::Open("1.0".into(), "2.0".into()),
        ));
        data.push((
            "[1.0,2.0]",
            MavenVersionRange::Closed("1.0".into(), "2.0".into()),
        ));

        let le_1_0 = MavenVersionRange::LE("1.0".into());
        let ge_1_2 = MavenVersionRange::GE("1.2".into());
        data.push((
            "(,1.0],[1.2,)",
            MavenVersionRange::Multiple(vec![le_1_0, ge_1_2]),
        ));

        data.push(("(,1.1),(1.1,)", MavenVersionRange::NE("1.1".into())));

        data.push((
            "[1.0,2.0)",
            MavenVersionRange::LowerLimit("1.0".into(), "2.0".into()),
        ));

        for (s, v) in data {
            eprintln!("Testing {s}");

            assert_eq!(v, s.parse().unwrap());

            // special case for 1.0 because it is normalized to [1.0,)
            if s == "1.0" {
                assert_eq!("[1.0,)", v.to_string());
            } else {
                assert_eq!(s, v.to_string());
            }

            if s != "(,1.0],[1.2,)" && s != "(,1.1),(1.1,)" {
                assert!(VersionReq::try_from(v).is_ok());
            }
        }
    }
}
