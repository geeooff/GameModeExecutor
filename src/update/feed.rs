//! What the release answers, and what is read out of it.
//!
//! The only network the program ever touches, so it is behind a trait:
//! [`Feed`] answers three questions -- where does `releases/latest` redirect,
//! what does a small text file say, and put this asset in that file --
//! and everything above it is pure. `WinHttp` is the one real feed;
//! the tests script one. Measured against `v0.1.0` on 2026-09-18:
//! `docs/design/13-updating.md` has the four answers.
//!
//! Every request after the check names the tag, never `latest`, so a
//! release published between the two cannot mix one version's checksum
//! with another's file.

use std::fmt;
use std::path::Path;

use super::Fault;

/// A version as three numbers, which is all a tag may carry: `v0.2.0`.
/// Anything else is "a release this version does not understand", never
/// a guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self(major, minor, patch))
    }

    /// The version this build carries, from `Cargo.toml` through
    /// `build_info`.
    pub fn running() -> Self {
        Self::parse(crate::build_info::PACKAGE_VERSION)
            .expect("Cargo.toml carries a three-part version")
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// A release found newer than the running version: enough to show, to
/// fetch and to verify.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// `v0.2.0`, as GitHub spells it in every URL that names the release.
    pub tag: String,
    /// The release page, for *What changed*.
    pub page: String,
    /// The asset to fetch: the installer for an installed copy, the zip for
    /// an unpacked one.
    pub asset: String,
    /// Its SHA-256, lower-case hex, from the release's `SHA256SUMS.txt`.
    pub sha256: String,
}

impl Release {
    pub fn download_url(&self, repository: &str) -> String {
        format!("{repository}/releases/download/{}/{}", self.tag, self.asset)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    UpToDate,
    Available(Release),
}

/// Which file a copy of the program updates itself with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Installer,
    Zip,
}

impl Kind {
    pub fn asset_name(self, version: Version) -> String {
        match self {
            Kind::Installer => format!("GameModeExecutor-{version}.msi"),
            Kind::Zip => format!("GameModeExecutor-{version}.zip"),
        }
    }
}

/// The three requests, and nothing else the program ever asks a network.
pub trait Feed {
    /// The `Location` a `HEAD` on `url` answers with, redirects *not*
    /// followed: for `releases/latest` that is the tag, and nothing to
    /// parse but a URL.
    fn redirect_of(&self, url: &str) -> Result<String, Fault>;
    /// A small text file, redirects followed.
    fn text(&self, url: &str) -> Result<String, Fault>;
    /// An asset written to `to`, redirects followed. `progress` is told the
    /// size once the headers are in. Returns the bytes written.
    fn download(
        &self,
        url: &str,
        to: &Path,
        progress: &mut dyn FnMut(Option<u64>),
    ) -> Result<u64, Fault>;
}

/// The tag at the end of `…/releases/tag/<tag>`, and the version in it.
/// A `Location` that points anywhere else -- a captive portal, a moved
/// repository -- is an unexpected answer, not a version.
pub fn parse_latest(location: &str, repository: &str) -> Result<(String, Version), Fault> {
    let prefix = format!("{repository}/releases/tag/");
    let tag = location
        .strip_prefix(&prefix)
        .map(|rest| rest.trim_end_matches('/'))
        .ok_or_else(|| Fault::Unexpected(format!("releases/latest redirected to {location}")))?;
    let version = tag
        .strip_prefix('v')
        .and_then(Version::parse)
        .ok_or_else(|| Fault::Unexpected(format!("the latest release is tagged {tag}")))?;
    Ok((tag.to_owned(), version))
}

/// The hash for `asset` in a `SHA256SUMS.txt`: `<hex>  <name>` per line, as
/// `sha256sum` writes it and the release workflow does.
pub fn parse_sums(text: &str, asset: &str) -> Result<String, Fault> {
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(hash), Some(name)) = (parts.next(), parts.next())
            && name == asset
        {
            if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Ok(hash.to_ascii_lowercase());
            }
            return Err(Fault::Unexpected(format!(
                "the checksum file carries no SHA-256 for {asset}"
            )));
        }
    }
    Err(Fault::Unexpected(format!(
        "the checksum file does not list {asset}"
    )))
}

/// The check: one request when there is nothing newer, two when there is.
pub fn check(
    feed: &dyn Feed,
    repository: &str,
    running: Version,
    kind: Kind,
) -> Result<Verdict, Fault> {
    let location = feed.redirect_of(&format!("{repository}/releases/latest"))?;
    let (tag, version) = parse_latest(&location, repository)?;
    if version <= running {
        return Ok(Verdict::UpToDate);
    }
    let asset = kind.asset_name(version);
    let sums = feed.text(&format!(
        "{repository}/releases/download/{tag}/SHA256SUMS.txt"
    ))?;
    let sha256 = parse_sums(&sums, &asset)?;
    Ok(Verdict::Available(Release {
        version,
        page: format!("{repository}/releases/tag/{tag}"),
        tag,
        asset,
        sha256,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str = "https://github.com/Geeooff/GameModeExecutor";

    #[test]
    fn versions_are_three_numbers_and_nothing_else() {
        assert_eq!(Version::parse("0.1.0"), Some(Version(0, 1, 0)));
        assert_eq!(Version::parse("10.2.33"), Some(Version(10, 2, 33)));
        assert_eq!(Version::parse("0.2.0-rc1"), None);
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert!(Version(0, 2, 0) > Version(0, 1, 9));
        assert!(Version(1, 0, 0) > Version(0, 99, 99));
        assert_eq!(Version(0, 1, 0).to_string(), "0.1.0");
    }

    #[test]
    fn the_tag_is_read_from_the_redirect_and_nowhere_else() {
        let (tag, version) = parse_latest(&format!("{REPO}/releases/tag/v0.2.0"), REPO).unwrap();
        assert_eq!(tag, "v0.2.0");
        assert_eq!(version, Version(0, 2, 0));

        let portal = parse_latest("https://login.example.net/?next=github", REPO);
        assert!(matches!(portal, Err(Fault::Unexpected(_))));
        let odd = parse_latest(&format!("{REPO}/releases/tag/nightly"), REPO);
        assert!(matches!(odd, Err(Fault::Unexpected(_))));
    }

    #[test]
    fn the_checksum_file_is_read_by_asset_name() {
        let sums = "85d6178b6133e056309cceef1187d403b222429c4be26158681aff5772d0d5d1  GameModeExecutor-0.1.0.msi\n\
                    b5b1618bd1d1223a4c9da3ce9df3ed8f0d2a820d4f04e3544d225ea278ae316a  GameModeExecutor-0.1.0.zip\n";
        assert_eq!(
            parse_sums(sums, "GameModeExecutor-0.1.0.zip").unwrap(),
            "b5b1618bd1d1223a4c9da3ce9df3ed8f0d2a820d4f04e3544d225ea278ae316a"
        );
        assert!(matches!(
            parse_sums(sums, "GameModeExecutor-0.1.0.exe"),
            Err(Fault::Unexpected(_))
        ));
        assert!(matches!(
            parse_sums("<html>captive portal</html>", "GameModeExecutor-0.1.0.msi"),
            Err(Fault::Unexpected(_))
        ));
        assert!(matches!(
            parse_sums(
                "notahash  GameModeExecutor-0.1.0.msi",
                "GameModeExecutor-0.1.0.msi"
            ),
            Err(Fault::Unexpected(_))
        ));
    }

    /// A feed that answers from a script.
    pub(crate) struct Scripted {
        pub latest: Result<String, Fault>,
        pub sums: Result<String, Fault>,
    }

    impl Feed for Scripted {
        fn redirect_of(&self, _url: &str) -> Result<String, Fault> {
            self.latest.clone()
        }
        fn text(&self, _url: &str) -> Result<String, Fault> {
            self.sums.clone()
        }
        fn download(
            &self,
            _url: &str,
            _to: &Path,
            _progress: &mut dyn FnMut(Option<u64>),
        ) -> Result<u64, Fault> {
            unreachable!("the check never downloads")
        }
    }

    #[test]
    fn an_older_or_equal_release_is_up_to_date_after_one_request() {
        let feed = Scripted {
            latest: Ok(format!("{REPO}/releases/tag/v0.1.0")),
            sums: Err(Fault::Unexpected("should not be asked".into())),
        };
        assert_eq!(
            check(&feed, REPO, Version(0, 1, 0), Kind::Installer).unwrap(),
            Verdict::UpToDate
        );
        assert_eq!(
            check(&feed, REPO, Version(0, 3, 0), Kind::Installer).unwrap(),
            Verdict::UpToDate,
            "a downgrade is never offered"
        );
    }

    #[test]
    fn a_newer_release_carries_its_asset_and_hash() {
        let feed = Scripted {
            latest: Ok(format!("{REPO}/releases/tag/v0.2.0")),
            sums: Ok("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789  GameModeExecutor-0.2.0.msi\n\
                      0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  GameModeExecutor-0.2.0.zip\n".to_owned()),
        };
        let verdict = check(&feed, REPO, Version(0, 1, 0), Kind::Zip).unwrap();
        let Verdict::Available(release) = verdict else {
            panic!("expected a release");
        };
        assert_eq!(release.version, Version(0, 2, 0));
        assert_eq!(release.tag, "v0.2.0");
        assert_eq!(release.asset, "GameModeExecutor-0.2.0.zip");
        assert_eq!(
            release.sha256,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
        assert_eq!(release.page, format!("{REPO}/releases/tag/v0.2.0"));
        assert_eq!(
            release.download_url(REPO),
            format!("{REPO}/releases/download/v0.2.0/GameModeExecutor-0.2.0.zip")
        );
    }

    #[test]
    fn a_network_fault_is_passed_through_unchanged() {
        let feed = Scripted {
            latest: Err(Fault::NoConnection { code: 12007 }),
            sums: Ok(String::new()),
        };
        assert_eq!(
            check(&feed, REPO, Version(0, 1, 0), Kind::Installer),
            Err(Fault::NoConnection { code: 12007 })
        );
    }
}
