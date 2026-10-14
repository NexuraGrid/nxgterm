//! Release versions: `x.y.z` with an optional `-pre.release` suffix.

use std::cmp::Ordering;
use std::fmt;

/// A release version, ordered like SemVer (a pre-release sorts before
/// its release; build metadata is not used by nxgterm releases).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    /// The dot-separated identifiers after `-`; empty for a release.
    pre: Vec<String>,
}

impl Version {
    /// Parses `1.2.3` or `1.2.3-rc.1`, with an optional leading `v`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let mut numbers = core.split('.').map(number);
        let (major, minor, patch) = (numbers.next()??, numbers.next()??, numbers.next()??);
        if numbers.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => Vec::new(),
            Some(pre) => {
                let ids: Vec<String> = pre.split('.').map(str::to_owned).collect();
                let valid = |id: &String| {
                    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                };
                if !ids.iter().all(valid) {
                    return None;
                }
                ids
            }
        };
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let core = |v: &Self| (v.major, v.minor, v.patch);
        core(self).cmp(&core(other)).then_with(|| {
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => compare_pre(&self.pre, &other.pre),
            }
        })
    }
}

/// A version number: ASCII digits only (no sign, no spaces).
fn number(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// SemVer pre-release order: identifier by identifier, numbers
/// numerically and before words, a shorter prefix first.
fn compare_pre(a: &[String], b: &[String]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        let order = match (number(x), number(y)) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => x.cmp(y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap_or_else(|| panic!("{text} should parse"))
    }

    #[test]
    fn parses_releases_and_pre_releases() {
        assert_eq!(v("0.10.1").to_string(), "0.10.1");
        assert_eq!(v("v1.2.3").to_string(), "1.2.3");
        assert_eq!(v("1.2.3-rc.1").to_string(), "1.2.3-rc.1");
    }

    #[test]
    fn rejects_malformed_versions() {
        for text in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "1.x.3",
            "1.2.3-",
            "1.2.3-a..b",
            "-1.2.3",
            "1.2.3 ",
        ] {
            assert_eq!(Version::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn orders_numerically() {
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.10.2") > v("0.10.1"));
        assert_eq!(v("0.10.1").cmp(&v("v0.10.1")), Ordering::Equal);
    }

    #[test]
    fn pre_releases_sort_before_their_release() {
        assert!(v("1.0.0-rc.1") < v("1.0.0"));
        assert!(v("1.0.0-rc.1") > v("0.9.0"));
        assert!(v("1.0.0-alpha") < v("1.0.0-alpha.1"));
        assert!(v("1.0.0-alpha.1") < v("1.0.0-beta"));
        assert!(v("1.0.0-rc.2") < v("1.0.0-rc.10"));
        assert!(v("1.0.0-1") < v("1.0.0-alpha"));
    }
}
