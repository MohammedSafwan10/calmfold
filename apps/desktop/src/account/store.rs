//! `account.json` in the data folder: the session token, the Secret Key per
//! email and a few public values (salt, wrapped account key, last plan) so
//! the account page and "Unlock Ottero Cloud" work offline.
//!
//! The file is written atomically with 0600 permissions on Unix, in debug
//! and release builds. Development builds never use the system keychain
//! (docs/security.md, Keychain policy); release builds move the token and
//! Secret Key to the OS keychain in a later step. The account key itself is
//! never written: it exists only in memory while unlocked.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use jiff::Timestamp;
use ottero_crypto::Zeroizing;
use serde::{Deserialize, Serialize};

use super::client::{Cloud, Me, SessionToken};

const VERSION: u32 = 1;

/// The signed-in account on this device.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SignedIn {
    pub email: String,
    pub token: String,
    /// Public KDF salt (base64url).
    pub salt: String,
    /// The account key wrapped by the password (ciphertext, opaque).
    #[serde(default)]
    pub wrapped_by_password: Option<String>,
    #[serde(default)]
    pub plan: Option<PlanRecord>,
}

/// The last plan seen from `/api/me`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct PlanRecord {
    pub role: String,
    #[serde(default)]
    pub cloud_source: Option<String>,
    #[serde(default)]
    pub cloud_ends_at: Option<Timestamp>,
}

impl PlanRecord {
    #[must_use]
    pub fn from_me(me: &Me) -> Self {
        Self {
            role: me.role.clone(),
            cloud_source: me.cloud.as_ref().map(|c| c.source.clone()),
            cloud_ends_at: me.cloud.as_ref().and_then(|c| c.ends_at),
        }
    }

    #[must_use]
    pub fn plan(&self) -> ottero_core::account::Plan {
        ottero_core::account::Plan::from_parts(
            &self.role,
            self.cloud_source
                .as_deref()
                .map(|s| (s, self.cloud_ends_at)),
        )
    }

    #[must_use]
    pub fn cloud(&self) -> Option<Cloud> {
        self.cloud_source.as_ref().map(|s| Cloud {
            source: s.clone(),
            status: "active".into(),
            ends_at: self.cloud_ends_at,
        })
    }
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
struct Data {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    account: Option<SignedIn>,
    /// Normalized email → Secret Key (`O1-…`). Kept after sign-out so
    /// signing in again on this device only needs the password.
    #[serde(default)]
    secret_keys: BTreeMap<String, String>,
}

/// The file and its parsed contents. `None` path keeps everything in memory
/// (tests, or a library without a data folder).
pub struct AccountFile {
    path: Option<PathBuf>,
    data: Data,
}

impl AccountFile {
    #[must_use]
    pub fn in_memory() -> Self {
        Self {
            path: None,
            data: Data::default(),
        }
    }

    /// Reads `path`. A damaged file is moved aside (never overwritten, it
    /// may hold the only copy of a Secret Key) and Ottero starts signed out.
    #[must_use]
    pub fn open(path: PathBuf) -> Self {
        let data = match std::fs::read(&path) {
            Ok(bytes) => {
                let bytes = Zeroizing::new(bytes);
                match serde_json::from_slice::<Data>(&bytes) {
                    Ok(data) => data,
                    Err(_) => {
                        let aside = path.with_extension(format!(
                            "damaged-{}.json",
                            Timestamp::now().as_second()
                        ));
                        eprintln!(
                            "ottero: account.json is unreadable; kept it as {}",
                            aside.display()
                        );
                        _ = std::fs::rename(&path, &aside);
                        Data::default()
                    }
                }
            }
            Err(_) => Data::default(),
        };
        Self {
            path: Some(path),
            data,
        }
    }

    #[must_use]
    pub fn signed_in(&self) -> Option<&SignedIn> {
        self.data.account.as_ref()
    }

    #[must_use]
    pub fn token(&self) -> Option<SessionToken> {
        self.signed_in().map(|a| SessionToken::new(a.token.clone()))
    }

    /// The stored Secret Key text for `email` (normalized), if any.
    #[must_use]
    pub fn secret_key(&self, email: &str) -> Option<Zeroizing<String>> {
        let email = ottero_core::account::normalize_email(email);
        self.data
            .secret_keys
            .get(&email)
            .map(|k| Zeroizing::new(k.clone()))
    }

    pub fn set_secret_key(&mut self, email: &str, key: &str) -> Result<()> {
        let email = ottero_core::account::normalize_email(email);
        self.data.secret_keys.insert(email, key.to_owned());
        self.save()
    }

    pub fn set_signed_in(&mut self, account: SignedIn) -> Result<()> {
        self.data.account = Some(account);
        self.save()
    }

    pub fn update_signed_in(&mut self, f: impl FnOnce(&mut SignedIn)) -> Result<()> {
        if let Some(account) = self.data.account.as_mut() {
            f(account);
            self.save()?;
        }
        Ok(())
    }

    /// Forgets the session (the Secret Key stays).
    pub fn sign_out(&mut self) -> Result<()> {
        self.data.account = None;
        self.save()
    }

    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn save(&mut self) -> Result<()> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        self.data.version = VERSION;
        let bytes = Zeroizing::new(serde_json::to_vec_pretty(&self.data)?);
        write_private(&path, &bytes)
            .context("Couldn't save your Ottero Cloud sign-in on this computer.")
    }
}

/// Writes `bytes` to a sibling temp file readable only by the user, syncs
/// it, then renames it over `path`.
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("no folder")?;
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("json.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    #[cfg(unix)]
    {
        // `mode` only applies to new files; tighten a leftover temp file too.
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> SignedIn {
        SignedIn {
            email: "sam@example.com".into(),
            token: "tok.sig".into(),
            salt: "B465J6Y1apD8t4WQw10Jzg".into(),
            wrapped_by_password: Some("wrapped".into()),
            plan: Some(PlanRecord {
                role: "user".into(),
                cloud_source: Some("grant".into()),
                cloud_ends_at: None,
            }),
        }
    }

    #[test]
    fn round_trips_and_keeps_the_secret_key_after_sign_out() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("account.json");
        let mut file = AccountFile::open(path.clone());
        assert!(file.signed_in().is_none());
        file.set_secret_key(" Sam@Example.com", "O1-AAAAAA-BBBBB-CCCCC-DDDDD-EEEEE")
            .expect("save");
        file.set_signed_in(account()).expect("save");

        let reopened = AccountFile::open(path.clone());
        assert!(reopened.signed_in() == Some(&account()));
        assert_eq!(
            reopened
                .secret_key("sam@example.com")
                .as_deref()
                .map(String::as_str),
            Some("O1-AAAAAA-BBBBB-CCCCC-DDDDD-EEEEE")
        );
        assert!(reopened.secret_key("other@example.com").is_none());

        let mut file = reopened;
        file.sign_out().expect("sign out");
        let reopened = AccountFile::open(path);
        assert!(reopened.signed_in().is_none());
        assert!(reopened.secret_key("sam@example.com").is_some());
    }

    #[cfg(unix)]
    #[test]
    fn file_is_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("account.json");
        let mut file = AccountFile::open(path.clone());
        file.set_signed_in(account()).expect("save");
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn damaged_file_is_kept_aside_not_overwritten() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("account.json");
        std::fs::write(&path, b"{not json").expect("write");
        let file = AccountFile::open(path.clone());
        assert!(file.signed_in().is_none());
        assert!(!path.exists());
        let kept = std::fs::read_dir(dir.path())
            .expect("list")
            .filter_map(Result::ok)
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("account.damaged-")
            });
        assert!(kept);
    }
}
