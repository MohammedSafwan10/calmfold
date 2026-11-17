//! Where the two secrets of an Ottero Cloud sign-in live on this device: the
//! session token and the Secret Key for each email.
//!
//! * **Windows release builds:** Windows Credential Manager, persistence
//!   "Local" (this computer only), encrypted with the user's Windows login.
//! * **macOS, and every development build:** inside `account.json` (0600).
//!   macOS keychain items are tied to the app's code signature; Ottero's
//!   ad-hoc signature changes with every update, so each update would ask for
//!   the keychain password. The Data Protection keychain needs a Developer ID
//!   signature, which comes with release signing (docs/security.md, ADR 0012).
//!   Development builds never use the system store (Keychain policy).

use std::collections::BTreeMap;

use anyhow::Result;
use ottero_crypto::Zeroizing;
use serde::{Deserialize, Serialize};

/// The secret half of `account.json`.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
pub struct Secrets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Normalized email → Secret Key (`O1-…`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secret_keys: BTreeMap<String, String>,
}

impl Secrets {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.token.is_none() && self.secret_keys.is_empty()
    }
}

/// A store for [`Secrets`] outside the file. `None` from [`SecretSlot::open`]
/// means the secrets stay in `account.json`.
pub trait SecretSlot: Send {
    /// What the store holds now (empty if nothing was saved).
    fn load(&self) -> Result<Secrets>;
    /// Replaces what the store holds; empty secrets delete the entry.
    fn save(&self, secrets: &Secrets) -> Result<()>;
}

/// The platform store for this build, if it has one.
#[must_use]
pub fn platform_slot() -> Option<Box<dyn SecretSlot>> {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        match windows::CredentialSlot::open() {
            Ok(slot) => return Some(Box::new(slot)),
            Err(err) => eprintln!(
                "ottero: Windows Credential Manager unavailable ({err}); keeping the sign-in in account.json"
            ),
        }
    }
    None
}

/// The JSON a slot stores, zeroized after use.
fn encode(secrets: &Secrets) -> Result<Zeroizing<Vec<u8>>> {
    Ok(Zeroizing::new(serde_json::to_vec(secrets)?))
}

fn decode(bytes: &[u8]) -> Result<Secrets> {
    Ok(serde_json::from_slice(bytes)?)
}

#[cfg(windows)]
pub mod windows {
    //! Windows Credential Manager, one generic credential named
    //! `Ottero Cloud sign-in`, persistence Local.
    use std::collections::HashMap;

    use anyhow::{Context as _, Result};
    use keyring_core::{Entry, Error};

    use super::{SecretSlot, Secrets, decode, encode};

    const SERVICE: &str = "Ottero Cloud sign-in";

    pub struct CredentialSlot {
        entry: Entry,
    }

    impl CredentialSlot {
        pub fn open() -> Result<Self> {
            Self::named(SERVICE)
        }

        /// A slot with another name (tests use a throwaway one).
        pub fn named(service: &str) -> Result<Self> {
            let store = windows_native_keyring_store::Store::new().context("Credential Manager")?;
            let mods = HashMap::from([("persistence", "Local")]);
            let entry = keyring_core::api::CredentialStoreApi::build(
                &*store,
                service,
                "account",
                Some(&mods),
            )
            .context("Credential Manager entry")?;
            Ok(Self { entry })
        }
    }

    impl SecretSlot for CredentialSlot {
        fn load(&self) -> Result<Secrets> {
            match self.entry.get_secret() {
                Ok(bytes) => decode(&ottero_crypto::Zeroizing::new(bytes)),
                Err(Error::NoEntry) => Ok(Secrets::default()),
                Err(err) => Err(err)
                    .context("Couldn't read the Ottero Cloud sign-in from Credential Manager"),
            }
        }

        fn save(&self, secrets: &Secrets) -> Result<()> {
            if secrets.is_empty() {
                return match self.entry.delete_credential() {
                    Ok(()) | Err(Error::NoEntry) => Ok(()),
                    Err(err) => Err(err).context(
                        "Couldn't remove the Ottero Cloud sign-in from Credential Manager",
                    ),
                };
            }
            self.entry
                .set_secret(&encode(secrets)?)
                .context("Couldn't save the Ottero Cloud sign-in to Credential Manager")
        }
    }
}

/// An in-memory slot standing in for a platform store in tests.
#[derive(Default)]
pub struct MemorySlot {
    pub held: std::sync::Mutex<Option<Zeroizing<Vec<u8>>>>,
}

impl SecretSlot for MemorySlot {
    fn load(&self) -> Result<Secrets> {
        let held = self.held.lock().map_err(|_| anyhow::anyhow!("poisoned"))?;
        held.as_deref()
            .map_or_else(|| Ok(Secrets::default()), |b| decode(b))
    }

    fn save(&self, secrets: &Secrets) -> Result<()> {
        let mut held = self.held.lock().map_err(|_| anyhow::anyhow!("poisoned"))?;
        *held = if secrets.is_empty() {
            None
        } else {
            Some(encode(secrets)?)
        };
        Ok(())
    }
}

impl<T: SecretSlot + Sync> SecretSlot for std::sync::Arc<T> {
    fn load(&self) -> Result<Secrets> {
        (**self).load()
    }
    fn save(&self, secrets: &Secrets) -> Result<()> {
        (**self).save(secrets)
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    /// Touches the real Credential Manager, so it only runs when CI opts in
    /// (development builds otherwise never use the system store).
    #[test]
    fn credential_manager_round_trip() {
        if std::env::var("OTTERO_TEST_WINDOWS_CREDENTIALS").map_or(true, |v| v != "1") {
            return;
        }
        let slot =
            windows::CredentialSlot::named("Ottero test sign-in (safe to delete)").expect("slot");
        let mut secrets = Secrets {
            token: Some("tok.sig".into()),
            ..Default::default()
        };
        secrets.secret_keys.insert(
            "sam@example.com".into(),
            "O1-AAAAAA-BBBBB-CCCCC-DDDDD-EEEEE".into(),
        );
        slot.save(&secrets).expect("save");
        assert!(slot.load().expect("load") == secrets);
        slot.save(&Secrets::default()).expect("delete");
        assert!(slot.load().expect("load").is_empty());
    }
}
