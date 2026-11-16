//! The update feed and the rules for trusting it. No IO: the desktop app
//! downloads the bytes, this module decides.
//!
//! A release publishes `stable.json` (version, notes, and per platform a
//! download URL, SHA-256 and size) and `stable.json.minisig`, signed with
//! Calmfold's release key. A download is trusted only when
//! 1. the feed's minisign signature verifies with the public key built into
//!    the app ([`RELEASE_KEY`]),
//! 2. its version is newer than the running one, and
//! 3. the downloaded file's size and SHA-256 match the feed.
//!
//! The app checks at most once a day ([`CHECK_EVERY`]) and installs only on
//! restart, so nothing changes under a user who is working.

use jiff::{SignedDuration, Timestamp};
use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Calmfold's release signing key (`minisign`, key id D46787DCA0DB66A3).
/// The secret half never leaves the release machine and CI.
pub const RELEASE_KEY: &str = "RWSjZtug3Idn1JE3I3J+oLz/+Gm43DS0PSppGsO3rBrnt4L7eBtFluOz";

/// Where the feed lives (a public repository that holds only release files).
pub const FEED_URL: &str =
    "https://github.com/MohammedSafwan10/calmfold-releases/releases/latest/download/stable.json";

/// How often to check on its own.
pub const CHECK_EVERY: SignedDuration = SignedDuration::from_hours(24);

/// Largest feed and download accepted (keeps a broken server bounded).
pub const MAX_FEED_BYTES: usize = 64 * 1024;
pub const MAX_DOWNLOAD_BYTES: u64 = 300 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Download {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feed {
    pub version: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub notes: Vec<String>,
    pub platforms: std::collections::BTreeMap<String, Download>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UpdateError {
    #[error("The update information isn't signed by Calmfold.")]
    BadSignature,
    #[error("The update information couldn't be read.")]
    BadFeed,
    #[error("The download doesn't match the release. It was not installed.")]
    BadDownload,
}

impl Feed {
    /// Parses the feed after checking its signature with `public_key`.
    pub fn verified(bytes: &[u8], signature: &str, public_key: &str) -> Result<Self, UpdateError> {
        if bytes.len() > MAX_FEED_BYTES {
            return Err(UpdateError::BadFeed);
        }
        let key = PublicKey::from_base64(public_key).map_err(|_| UpdateError::BadSignature)?;
        let sig = Signature::decode(signature).map_err(|_| UpdateError::BadSignature)?;
        key.verify(bytes, &sig, false)
            .map_err(|_| UpdateError::BadSignature)?;
        let feed: Self = serde_json::from_slice(bytes).map_err(|_| UpdateError::BadFeed)?;
        if parse_version(&feed.version).is_none() {
            return Err(UpdateError::BadFeed);
        }
        Ok(feed)
    }

    /// The download for `platform` (see [`platform`]) when it's newer than
    /// `current`.
    #[must_use]
    pub fn newer_for(&self, current: &str, platform: &str) -> Option<&Download> {
        is_newer(&self.version, current)
            .then(|| self.platforms.get(platform))
            .flatten()
            .filter(|d| d.url.starts_with("https://") && d.size <= MAX_DOWNLOAD_BYTES)
    }
}

/// `major.minor.patch` of a plain release version ("0.1.2", "v1.0.0").
#[must_use]
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.trim().trim_start_matches('v').split('.');
    let out = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(out)
}

/// Whether `candidate` is a later release than `current`.
#[must_use]
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// Whether a check is due: never checked, a day has passed, or the clock
/// went backwards.
#[must_use]
pub fn check_due(last: Option<Timestamp>, now: Timestamp) -> bool {
    last.is_none_or(|t| now < t || now.duration_since(t) >= CHECK_EVERY)
}

/// Checks a downloaded file against the feed's size and SHA-256.
pub fn verify_download(bytes: &[u8], expected: &Download) -> Result<(), UpdateError> {
    let digest = Sha256::digest(bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if bytes.len() as u64 == expected.size && hex.eq_ignore_ascii_case(&expected.sha256) {
        Ok(())
    } else {
        Err(UpdateError::BadDownload)
    }
}

/// The feed key for this build: `windows-x64`, `macos-arm64`, … or `None`
/// where Calmfold has no release build.
#[must_use]
pub fn platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("windows-x64"),
        ("macos", "aarch64") => Some("macos-arm64"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &[u8] = include_bytes!("../tests/data/feed.json");
    const SIG: &str = include_str!("../tests/data/feed.json.minisig");
    const TEST_KEY: &str = include_str!("../tests/data/test.pub");

    #[test]
    fn signed_feeds_parse_and_tampered_ones_do_not() {
        let feed = Feed::verified(FEED, SIG, TEST_KEY.trim()).expect("valid");
        assert_eq!(feed.version, "0.2.0");
        assert_eq!(feed.notes, ["Faster search."]);
        let mut tampered = FEED.to_vec();
        if let Some(b) = tampered.iter_mut().find(|b| **b == b'2') {
            *b = b'9';
        }
        assert_eq!(
            Feed::verified(&tampered, SIG, TEST_KEY.trim()),
            Err(UpdateError::BadSignature)
        );
        // Signed by a different key (the real release key didn't sign it).
        assert_eq!(
            Feed::verified(FEED, SIG, RELEASE_KEY),
            Err(UpdateError::BadSignature)
        );
        assert_eq!(
            Feed::verified(FEED, "junk", TEST_KEY.trim()),
            Err(UpdateError::BadSignature)
        );
    }

    #[test]
    fn only_newer_versions_for_this_platform_are_offered() {
        let feed = Feed::verified(FEED, SIG, TEST_KEY.trim()).expect("valid");
        assert!(feed.newer_for("0.1.1", "windows-x64").is_some());
        assert!(feed.newer_for("0.2.0", "windows-x64").is_none(), "same");
        assert!(feed.newer_for("0.3.0", "windows-x64").is_none(), "older");
        assert!(feed.newer_for("0.1.1", "linux-x64").is_none(), "no build");
        assert!(is_newer("0.10.0", "0.9.9"), "numeric, not text");
        assert!(is_newer("v1.0.0", "0.9.0"));
        assert!(!is_newer("1.0", "0.9.0"), "three parts only");
        assert!(!is_newer("1.0.0-beta", "0.9.0"));
    }

    #[test]
    fn downloads_must_match_size_and_hash() {
        let feed = Feed::verified(FEED, SIG, TEST_KEY.trim()).expect("valid");
        let d = &feed.platforms["windows-x64"];
        assert_eq!(verify_download(b"hello", d), Ok(()));
        assert_eq!(verify_download(b"hellO", d), Err(UpdateError::BadDownload));
        assert_eq!(verify_download(b"hello!", d), Err(UpdateError::BadDownload));
    }

    #[test]
    fn checks_run_once_a_day() {
        let t: Timestamp = "2026-11-14T09:00:00Z".parse().expect("ts");
        assert!(check_due(None, t));
        let later = t + SignedDuration::from_hours(23);
        assert!(!check_due(Some(t), later));
        assert!(check_due(Some(t), t + SignedDuration::from_hours(24)));
        assert!(check_due(Some(later), t), "clock went back");
    }
}
