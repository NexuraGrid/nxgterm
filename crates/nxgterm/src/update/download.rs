//! Fetching a release: its `SHA256SUMS`, then assets, each verified
//! against that file before anything uses it.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use ureq::Agent;
use ureq::config::Config;

use super::sums::Sums;
use super::version::Version;

/// The GitHub repository releases come from.
const RELEASES: &str = "https://github.com/NexuraGrid/nxgterm/releases";

/// `SHA256SUMS` is a few hundred bytes; anything this big is not one.
const MAX_SUMS: u64 = 1 << 20;

/// The largest asset accepted (the biggest, the macOS dmg, is far below).
const MAX_ASSET: u64 = 1 << 30;

/// Where releases are read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The latest GitHub release.
    GitHub,
    /// A directory URL holding `SHA256SUMS` and the assets
    /// (`NXGTERM_UPDATE_URL`).
    Mirror(String),
}

impl Source {
    /// `NXGTERM_UPDATE_URL` when set, else GitHub.
    pub fn from_env() -> Self {
        match std::env::var("NXGTERM_UPDATE_URL") {
            Ok(url) if !url.trim().is_empty() => {
                Self::Mirror(url.trim().trim_end_matches('/').to_owned())
            }
            _ => Self::GitHub,
        }
    }

    fn sums_url(&self) -> String {
        match self {
            Self::GitHub => format!("{RELEASES}/latest/download/SHA256SUMS"),
            Self::Mirror(base) => format!("{base}/SHA256SUMS"),
        }
    }

    /// The asset's URL in the release tagged `v<version>`: pinned to
    /// that version, so a release published meanwhile cannot swap it.
    fn asset_url(&self, version: &Version, name: &str) -> String {
        match self {
            Self::GitHub => format!("{RELEASES}/download/v{version}/{name}"),
            Self::Mirror(base) => format!("{base}/{name}"),
        }
    }
}

/// The latest release: its version and asset checksums.
#[derive(Debug)]
pub struct Release {
    pub version: Version,
    pub sums: Sums,
}

/// An HTTP(S) client for one [`Source`].
#[derive(Debug)]
pub struct Client {
    agent: Agent,
    source: Source,
}

impl Client {
    /// A client honouring `HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY` and
    /// `NO_PROXY` (ureq reads them).
    pub fn new(source: Source) -> Self {
        Self::build(source, true)
    }

    /// `proxy: false` connects directly whatever the environment says.
    fn build(source: Source, proxy: bool) -> Self {
        let mut builder = Agent::config_builder()
            .user_agent(concat!("nxgterm/", env!("CARGO_PKG_VERSION")))
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .timeout_recv_body(Some(Duration::from_secs(600)));
        if !proxy {
            builder = builder.proxy(None);
        }
        let config: Config = builder.build();
        Self {
            agent: config.new_agent(),
            source,
        }
    }

    /// Reads the latest release's `SHA256SUMS`.
    pub fn latest(&self) -> Result<Release, String> {
        let url = self.source.sums_url();
        let mut text = String::new();
        self.get(&url, MAX_SUMS)?
            .read_to_string(&mut text)
            .map_err(|error| format!("cannot read {url}: {error}"))?;
        let sums = Sums::parse(&text).map_err(|error| format!("{url}: {error}"))?;
        let version = sums
            .version()
            .ok_or_else(|| format!("{url} names no nxgterm-<version>-source.tar.gz"))?;
        Ok(Release { version, sums })
    }

    /// Downloads asset `name` of `release` into `dir` and returns its
    /// path, only once its SHA-256 matches `SHA256SUMS`; a mismatch or a
    /// missing entry leaves nothing behind.
    pub fn download(&self, release: &Release, name: &str, dir: &Path) -> Result<PathBuf, String> {
        let expected = release
            .sums
            .digest(name)
            .ok_or_else(|| format!("SHA256SUMS of {} has no entry for {name}", release.version))?;
        let url = self.source.asset_url(&release.version, name);
        eprintln!("nxgterm: downloading {url}");
        let part = dir.join(format!("{name}.part"));
        let digest = self.get(&url, MAX_ASSET).and_then(|mut body| {
            save(&mut body, &part).map_err(|error| format!("cannot download {url}: {error}"))
        });
        let digest = match digest {
            Ok(digest) => digest,
            Err(error) => {
                let _ = fs::remove_file(&part);
                return Err(error);
            }
        };
        if digest != expected {
            let _ = fs::remove_file(&part);
            return Err(format!(
                "checksum mismatch for {name}: SHA256SUMS says {expected}, the download is {digest}; nothing was installed"
            ));
        }
        let path = dir.join(name);
        fs::rename(&part, &path)
            .map_err(|error| format!("cannot save {}: {error}", path.display()))?;
        eprintln!("nxgterm: verified the SHA-256 of {name}");
        Ok(path)
    }

    /// A successful response's body, at most `limit` bytes.
    fn get(&self, url: &str, limit: u64) -> Result<impl Read, String> {
        let response = self
            .agent
            .get(url)
            .call()
            .map_err(|error| format!("cannot download {url}: {error}"))?;
        Ok(response
            .into_body()
            .into_with_config()
            .limit(limit)
            .reader())
    }
}

/// Copies `body` to a new file at `path`, returning its SHA-256 (hex).
fn save(body: &mut impl Read, path: &Path) -> io::Result<String> {
    let mut file = File::create(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = match body.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])?;
    }
    file.sync_all()?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// The SHA-256 of `bytes`, lowercase hex.
#[cfg(test)]
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::test_dir;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::thread;

    /// Serves `files` (path, body) over plain HTTP on 127.0.0.1 until the
    /// test process exits; other paths are 404. Returns the base URL.
    fn serve(files: Vec<(String, Vec<u8>)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                let _ = reader.read_line(&mut request);
                loop {
                    let mut header = String::new();
                    match reader.read_line(&mut header) {
                        Ok(n) if n > 2 => {}
                        _ => break,
                    }
                }
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (status, body) = match files.iter().find(|(p, _)| p == path) {
                    Some((_, body)) => ("200 OK", body.clone()),
                    None => ("404 Not Found", b"not found".to_vec()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        format!("http://{address}/release")
    }

    /// A client for `base` that ignores any proxy of the test environment.
    fn client(base: String) -> Client {
        Client::build(Source::Mirror(base), false)
    }

    const GOOD: &[u8] = b"the real asset";
    const ASSET: &str = "nxgterm-9.9.9-x86_64-linux.tar.gz";
    const TAMPERED: &str = "nxgterm-9.9.9-x86_64-windows.zip";

    fn release_server() -> String {
        let sums = format!(
            "{}  nxgterm-9.9.9-source.tar.gz\n{}  {ASSET}\n{}  {TAMPERED}\n",
            sha256_hex(b"source"),
            sha256_hex(GOOD),
            sha256_hex(GOOD),
        );
        serve(vec![
            ("/release/SHA256SUMS".into(), sums.into_bytes()),
            (format!("/release/{ASSET}"), GOOD.to_vec()),
            (format!("/release/{TAMPERED}"), b"something else".to_vec()),
            ("/release/unlisted.msi".into(), GOOD.to_vec()),
        ])
    }

    #[test]
    fn downloads_and_verifies_an_asset() {
        let client = client(release_server());
        let release = client.latest().unwrap();
        assert_eq!(release.version, Version::parse("9.9.9").unwrap());
        let dir = test_dir("download-good");
        let path = client.download(&release, ASSET, &dir).unwrap();
        assert_eq!(path, dir.join(ASSET));
        assert_eq!(fs::read(&path).unwrap(), GOOD);
        assert!(!dir.join(format!("{ASSET}.part")).exists());
    }

    #[test]
    fn refuses_an_asset_whose_checksum_does_not_match() {
        let client = client(release_server());
        let release = client.latest().unwrap();
        let dir = test_dir("download-tampered");
        let error = client.download(&release, TAMPERED, &dir).unwrap_err();
        assert!(error.contains("checksum mismatch"), "{error}");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            0,
            "nothing is left behind"
        );
    }

    #[test]
    fn refuses_an_asset_missing_from_the_sums() {
        let client = client(release_server());
        let release = client.latest().unwrap();
        let dir = test_dir("download-unlisted");
        let error = client.download(&release, "unlisted.msi", &dir).unwrap_err();
        assert!(error.contains("no entry for unlisted.msi"), "{error}");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn http_errors_are_reported() {
        let client = client(serve(Vec::new()));
        let error = client.latest().unwrap_err();
        assert!(error.contains("SHA256SUMS"), "{error}");
        assert!(error.contains("404"), "{error}");
    }

    #[test]
    fn urls_pin_the_release_version() {
        let v = Version::parse("0.11.0").unwrap();
        assert_eq!(
            Source::GitHub.sums_url(),
            "https://github.com/NexuraGrid/nxgterm/releases/latest/download/SHA256SUMS"
        );
        assert_eq!(
            Source::GitHub.asset_url(&v, "a.zip"),
            "https://github.com/NexuraGrid/nxgterm/releases/download/v0.11.0/a.zip"
        );
        let mirror = Source::Mirror("http://h/d".into());
        assert_eq!(mirror.sums_url(), "http://h/d/SHA256SUMS");
        assert_eq!(mirror.asset_url(&v, "a.zip"), "http://h/d/a.zip");
    }
}
