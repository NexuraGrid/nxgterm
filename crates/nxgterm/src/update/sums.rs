//! The `SHA256SUMS` file of a release: `<sha256>  <asset name>` per line,
//! as written by `sha256sum`.

use super::version::Version;

/// The checksums of one release's assets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sums {
    /// (lowercase hex digest, asset name), in file order.
    entries: Vec<(String, String)>,
}

impl Sums {
    /// Parses `sha256sum` output; blank lines are skipped, a `*` before
    /// the name (binary mode) is dropped, anything else malformed fails.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut entries = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim_end_matches('\r');
            if line.trim().is_empty() {
                continue;
            }
            let malformed = || format!("SHA256SUMS line {} is malformed", index + 1);
            let (digest, name) = line.split_once(' ').ok_or_else(malformed)?;
            let name = name.trim_start_matches(' ');
            let name = name.strip_prefix('*').unwrap_or(name);
            let hex = digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit());
            // Asset names are plain file names; anything else is not ours.
            let plain =
                !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != "..";
            if !hex || !plain {
                return Err(malformed());
            }
            entries.push((digest.to_ascii_lowercase(), name.to_owned()));
        }
        Ok(Self { entries })
    }

    /// The expected digest of `name`, lowercase hex.
    pub fn digest(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(_, entry)| entry == name)
            .map(|(digest, _)| digest.as_str())
    }

    /// The release version, read from the `nxgterm-<v>-source.tar.gz`
    /// entry every release has.
    pub fn version(&self) -> Option<Version> {
        self.entries
            .iter()
            .filter_map(|(_, name)| {
                let version = name
                    .strip_prefix("nxgterm-")?
                    .strip_suffix("-source.tar.gz")?;
                Version::parse(version)
            })
            .max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";

    fn sample() -> String {
        format!(
            "{A}  nxgterm-0.11.0-source.tar.gz\n\n{B} *nxgterm-0.11.0-x86_64-windows.zip\r\n{A}  nxgterm_0.11.0-1_amd64.deb\n"
        )
    }

    #[test]
    fn looks_up_digests_by_name() {
        let sums = Sums::parse(&sample()).unwrap();
        assert_eq!(sums.digest("nxgterm-0.11.0-source.tar.gz"), Some(A));
        assert_eq!(
            sums.digest("nxgterm-0.11.0-x86_64-windows.zip"),
            Some(B.to_ascii_lowercase().as_str())
        );
        assert_eq!(sums.digest("nxgterm_0.11.0-1_amd64.deb"), Some(A));
        assert_eq!(sums.digest("nxgterm-0.11.0-x86_64.msi"), None);
    }

    #[test]
    fn reads_the_version_from_the_source_tarball() {
        let sums = Sums::parse(&sample()).unwrap();
        assert_eq!(sums.version(), Version::parse("0.11.0"));
        let pre = Sums::parse(&format!("{A}  nxgterm-1.0.0-rc.1-source.tar.gz\n")).unwrap();
        assert_eq!(pre.version(), Version::parse("1.0.0-rc.1"));
        let none = Sums::parse(&format!("{A}  nxgterm-0.11.0-x86_64.msi\n")).unwrap();
        assert_eq!(none.version(), None);
    }

    #[test]
    fn rejects_malformed_lines() {
        assert!(Sums::parse("not a checksum line\n").is_err());
        assert!(Sums::parse(&format!("{}  short.tar.gz\n", &A[1..])).is_err());
        assert!(Sums::parse(&format!("{}g  bad-hex\n", &A[1..])).is_err());
        assert!(Sums::parse(&format!("{A}\n")).is_err());
        assert!(Sums::parse(&format!("{A}  dir/name\n")).is_err());
    }
}
