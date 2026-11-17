//! `account.json` in the data folder: the session token, the Secret Key per
//! email and a few public values (salt, wrapped account key, last plan) so
//! the account page and "Unlock Ottero Cloud" work offline.
//!
//! The file is written atomically with 0600 permissions on Unix. The two
//! secrets (session token, Secret Keys) go to the platform store when the
//! build has one (`secrets::platform_slot`: Windows Credential Manager in
//! release builds) and stay in this file otherwise; a file written by an
//! older version has its secrets moved into the store on first open. The
//! account key itself is never written: it exists only in memory while
//! unlocked.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use jiff::Timestamp;
use ottero_crypto::Zeroizing;
use serde::{Deserialize, Serialize};

use super::client::{Cloud, Me, SessionToken};
use super::secrets::{SecretSlot, Secrets};

const VERSION: u32 = 1;

/// The signed-in account on this device.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SignedIn {
    pub email: String,
    /// Empty in the file when a platform store holds it.
    #[serde(default)]
    pub token: String,
    /// Public KDF salt (base64url).
    pub salt: String,
    /// The account key wrapped by the password (ciphertext, opaque).
    #[serde(default)]
    pub wrapped_by_password: Option<String>,
    #[serde(default)]
    pub plan: Option<PlanRecord>,
    /// Two-step verification on or off, as last seen from `/api/me`
    /// (`None` until the first answer), so the page can show it offline.
    #[serde(default)]
    pub two_factor: Option<bool>,
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
    /// Holds the token and Secret Keys instead of the file, when present.
    slot: Option<Box<dyn SecretSlot>>,
}

impl AccountFile {
    #[must_use]
    pub fn in_memory() -> Self {
        Self {
            path: None,
            data: Data::default(),
            slot: None,
        }
    }

    /// `account.json` at `path`, with secrets in this build's platform store.
    #[must_use]
    pub fn open(path: PathBuf) -> Self {
        Self::open_with(path, super::secrets::platform_slot())
    }

    /// Reads `path`. A damaged file is moved aside (never overwritten, it
    /// may hold the only copy of a Secret Key) and Ottero starts signed out.
    /// With a `slot`, secrets found in the file are moved into it.
    #[must_use]
    pub fn open_with(path: PathBuf, slot: Option<Box<dyn SecretSlot>>) -> Self {
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
        let mut file = Self {
            path: Some(path),
            data,
            slot,
        };
        file.attach_slot();
        file
    }

    /// Fills the in-memory copy from the slot and moves any secrets left in
    /// the file into it. If the slot can't be read, the sign-in still works
    /// from whatever the file holds; nothing is deleted.
    fn attach_slot(&mut self) {
        let Some(slot) = &self.slot else { return };
        let in_file = self.file_secrets();
        let stored = match slot.load() {
            Ok(s) => s,
            Err(err) => {
                eprintln!("ottero: {err:#}");
                self.slot = None;
                return;
            }
        };
        let mut merged = stored.clone();
        if merged.token.is_none() {
            merged.token = in_file.token.clone();
        }
        for (email, key) in &in_file.secret_keys {
            merged
                .secret_keys
                .entry(email.clone())
                .or_insert_with(|| key.clone());
        }
        if merged != stored && slot.save(&merged).is_err() {
            self.slot = None; // keep using the file rather than lose a Secret Key
            return;
        }
        self.apply_secrets(&merged);
        if !in_file.is_empty() {
            _ = self.save(); // rewrites the file without its secrets
        }
    }

    fn file_secrets(&self) -> Secrets {
        Secrets {
            token: self
                .data
                .account
                .as_ref()
                .map(|a| a.token.clone())
                .filter(|t| !t.is_empty()),
            secret_keys: self.data.secret_keys.clone(),
        }
    }

    fn apply_secrets(&mut self, secrets: &Secrets) {
        self.data.secret_keys = secrets.secret_keys.clone();
        match (&mut self.data.account, &secrets.token) {
            (Some(a), Some(t)) => a.token = t.clone(),
            // The token is gone from the store (removed by the user or another
            // copy): this device is signed out; the Secret Keys stay.
            (Some(_), None) => self.data.account = None,
            _ => {}
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
        let data = match &self.slot {
            Some(slot) => {
                // Secrets first, so a crash never leaves them only in memory.
                slot.save(&self.file_secrets())?;
                let mut public = self.data.clone();
                public.secret_keys.clear();
                if let Some(a) = public.account.as_mut() {
                    a.token = String::new();
                }
                public
            }
            None => self.data.clone(),
        };
        let bytes = Zeroizing::new(serde_json::to_vec_pretty(&data)?);
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
            two_factor: Some(false),
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

    fn slot() -> std::sync::Arc<super::super::secrets::MemorySlot> {
        std::sync::Arc::new(super::super::secrets::MemorySlot::default())
    }

    #[test]
    fn with_a_platform_store_the_file_holds_no_secrets() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("account.json");
        let store = slot();
        let mut file = AccountFile::open_with(path.clone(), Some(Box::new(store.clone())));
        file.set_secret_key("sam@example.com", "O1-AAAAAA-BBBBB-CCCCC-DDDDD-EEEEE")
            .expect("save");
        file.set_signed_in(account()).expect("save");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(!text.contains("tok.sig"), "token not in the file");
        assert!(!text.contains("O1-AAAAAA"), "Secret Key not in the file");
        assert!(
            text.contains("B465J6Y1apD8t4WQw10Jzg"),
            "public salt stays in the file"
        );

        let reopened = AccountFile::open_with(path.clone(), Some(Box::new(store.clone())));
        assert!(
            reopened.signed_in() == Some(&account()),
            "token comes back from the store"
        );
        assert!(reopened.secret_key("sam@example.com").is_some());

        let mut file = reopened;
        file.sign_out().expect("sign out");
        let held = store.load().expect("load");
        assert!(
            held.token.is_none(),
            "sign-out removes the token from the store"
        );
        assert_eq!(held.secret_keys.len(), 1, "the Secret Key stays");
    }

    #[test]
    fn secrets_from_an_older_file_move_into_the_store() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("account.json");
        let mut old = AccountFile::open_with(path.clone(), None);
        old.set_secret_key("sam@example.com", "O1-AAAAAA-BBBBB-CCCCC-DDDDD-EEEEE")
            .expect("save");
        old.set_signed_in(account()).expect("save");
        assert!(
            std::fs::read_to_string(&path)
                .expect("read")
                .contains("tok.sig")
        );

        let store = slot();
        let upgraded = AccountFile::open_with(path.clone(), Some(Box::new(store.clone())));
        assert!(upgraded.signed_in() == Some(&account()));
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(
            !text.contains("tok.sig") && !text.contains("O1-AAAAAA"),
            "file rewritten without secrets"
        );
        let held = store.load().expect("load");
        assert_eq!(held.token.as_deref(), Some("tok.sig"));
        assert!(held.secret_keys.contains_key("sam@example.com"));
    }

    #[test]
    fn an_unreadable_store_falls_back_to_the_file_without_losing_anything() {
        struct Broken;
        impl SecretSlot for Broken {
            fn load(&self) -> Result<Secrets> {
                anyhow::bail!("locked")
            }
            fn save(&self, _: &Secrets) -> Result<()> {
                anyhow::bail!("locked")
            }
        }
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("account.json");
        let mut old = AccountFile::open_with(path.clone(), None);
        old.set_signed_in(account()).expect("save");
        let file = AccountFile::open_with(path.clone(), Some(Box::new(Broken)));
        assert!(file.signed_in() == Some(&account()));
        assert!(
            std::fs::read_to_string(&path)
                .expect("read")
                .contains("tok.sig"),
            "nothing deleted"
        );
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
