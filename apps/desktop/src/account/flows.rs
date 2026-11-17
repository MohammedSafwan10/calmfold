//! The blocking steps of each account flow: network calls and key
//! derivation, in order. Every function here runs on a background thread;
//! `super::AccountSession` applies the results on the main thread.

use std::sync::Arc;

use ottero_crypto::account::{
    AccountKdf, AccountKeys, AccountSecretKey, RecoveryCode, derive_account_keys, encode_salt,
    unwrap_account_key, wrap_account_key, wrap_account_key_for_recovery,
};
use ottero_crypto::{CryptoError, Salt, SecretKey, SecretString, Zeroizing};

use super::client::{AccountApi, ApiError, Me, SessionToken, WrappedKeys};

/// A failure in a flow, before it is placed next to a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowError {
    Api(ApiError),
    /// The account key didn't open with these keys although the server
    /// accepted them: the stored key material is damaged.
    DamagedKeys,
    /// The password (with this device's Secret Key) doesn't open the account key.
    WrongPassword,
    /// The OS random generator or Argon2 failed.
    Crypto,
}

impl From<ApiError> for FlowError {
    fn from(err: ApiError) -> Self {
        Self::Api(err)
    }
}

impl FlowError {
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Api(err) => err.message(),
            Self::DamagedKeys => {
                "Your account's keys couldn't be opened. Contact support before trying again."
                    .into()
            }
            Self::WrongPassword => "That password isn't right.".into(),
            Self::Crypto => {
                "Ottero couldn't create secure keys on this computer. Try again.".into()
            }
        }
    }
}

pub type FlowResult<T> = Result<T, FlowError>;

/// Keys derived from the password for one email. Kept in memory between
/// steps (code entry, Emergency Kit) so the password isn't asked again.
pub struct Derived {
    pub email: String,
    pub keys: AccountKeys,
    pub secret_key: AccountSecretKey,
    pub salt: Salt,
}

/// A freshly created account key, its recovery code and both wraps,
/// waiting for the user to save the Emergency Kit.
pub struct Kit {
    pub account_key: SecretKey,
    pub recovery: RecoveryCode,
    pub wrapped: WrappedKeys,
}

/// Where a successful sign-in lands.
pub enum Finish {
    SignedIn {
        token: SessionToken,
        me: Me,
        account_key: SecretKey,
        wrapped_by_password: String,
    },
    /// Verified, but the keys were never saved: show the Emergency Kit.
    NeedsKit {
        token: SessionToken,
        me: Me,
        kit: Kit,
    },
}

fn derive(
    password: &SecretString,
    email: &str,
    salt: Salt,
    secret_key: AccountSecretKey,
    kdf: AccountKdf,
) -> FlowResult<Derived> {
    let keys = derive_account_keys(password, email, &salt, &secret_key, kdf)
        .map_err(|_| FlowError::Crypto)?;
    Ok(Derived {
        email: email.to_owned(),
        keys,
        secret_key,
        salt,
    })
}

/// Creates the account: new salt, derive, `sign-up`. The server then
/// emails the code. Reuses this device's Secret Key for the email if it
/// has one, so retrying never orphans a key.
pub fn sign_up(
    api: &dyn AccountApi,
    kdf: AccountKdf,
    email: &str,
    password: &SecretString,
    existing: Option<AccountSecretKey>,
) -> FlowResult<Derived> {
    let secret_key = match existing {
        Some(k) => k,
        None => AccountSecretKey::generate().map_err(|_| FlowError::Crypto)?,
    };
    let salt = Salt::generate().map_err(|_| FlowError::Crypto)?;
    let derived = derive(password, email, salt, secret_key, kdf)?;
    let name = email.split('@').next().unwrap_or(email);
    api.sign_up(
        email,
        name,
        &derived.keys.auth_key().to_hex(),
        &encode_salt(&derived.salt),
    )?;
    Ok(derived)
}

/// A new account key and recovery code, both wraps ready to upload.
pub fn new_kit(keys: &AccountKeys) -> FlowResult<Kit> {
    let account_key = SecretKey::generate().map_err(|_| FlowError::Crypto)?;
    let recovery = RecoveryCode::generate().map_err(|_| FlowError::Crypto)?;
    let wrapped = WrappedKeys {
        by_password: wrap_account_key(keys.wrap_key(), &account_key)
            .map_err(|_| FlowError::Crypto)?,
        by_recovery: wrap_account_key_for_recovery(&recovery, &account_key)
            .map_err(|_| FlowError::Crypto)?,
    };
    Ok(Kit {
        account_key,
        recovery,
        wrapped,
    })
}

/// Signs in with already-derived keys and opens (or creates) the account key.
pub fn finish_sign_in(api: &dyn AccountApi, derived: &Derived) -> FlowResult<Finish> {
    let token = api.sign_in(&derived.email, &derived.keys.auth_key().to_hex())?;
    let me = api.me(&token)?;
    match api.get_keys(&token)? {
        Some(wrapped) => {
            let account_key = unwrap_account_key(derived.keys.wrap_key(), &wrapped.by_password)
                .map_err(|_| FlowError::DamagedKeys)?;
            Ok(Finish::SignedIn {
                token,
                me,
                account_key,
                wrapped_by_password: wrapped.by_password,
            })
        }
        None => Ok(Finish::NeedsKit {
            token,
            me,
            kit: new_kit(&derived.keys)?,
        }),
    }
}

/// Sign-in: salt lookup, derive, then [`finish_sign_in`]. Also returns the
/// derived keys when they are needed next (an unverified account goes to
/// code entry with them; a code is resent).
pub fn sign_in(
    api: &dyn AccountApi,
    kdf_override: Option<AccountKdf>,
    email: &str,
    password: &SecretString,
    secret_key: AccountSecretKey,
) -> (Option<Arc<Derived>>, FlowResult<Finish>) {
    let info = match api.kdf(email) {
        Ok(info) => info,
        Err(err) => return (None, Err(err.into())),
    };
    let kdf = kdf_override.unwrap_or(info.kdf);
    let derived = match derive(password, email, info.salt, secret_key, kdf) {
        Ok(d) => Arc::new(d),
        Err(err) => return (None, Err(err)),
    };
    match finish_sign_in(api, &derived) {
        Err(FlowError::Api(ApiError::NotVerified)) => {
            // A fresh code, since the first one may have expired. If this
            // fails the code screen still offers "Resend code".
            _ = api.resend_code(email);
            (Some(derived), Err(FlowError::Api(ApiError::NotVerified)))
        }
        result => (Some(derived), result),
    }
}

/// Confirms the emailed code (unless an earlier attempt already did), then
/// signs in. Returns whether the email is now verified, so a retry after a
/// network failure skips the used code.
pub fn verify(
    api: &dyn AccountApi,
    derived: &Derived,
    code: &str,
    already_verified: bool,
) -> (bool, FlowResult<Finish>) {
    if !already_verified && let Err(err) = api.verify_email(&derived.email, code) {
        return (false, Err(err.into()));
    }
    (true, finish_sign_in(api, derived))
}

/// The account key that ended up on the server after saving the kit.
pub enum KitSaved {
    /// Our new key is stored.
    Saved,
    /// Another device saved keys first; this is that account key, and the
    /// recovery code just shown isn't valid.
    AlreadySet(SecretKey, String),
}

/// Uploads both wraps. Run only after the user confirmed saving the kit.
pub fn save_kit(
    api: &dyn AccountApi,
    token: &SessionToken,
    derived: &Derived,
    kit: &Kit,
) -> FlowResult<KitSaved> {
    match api.put_keys(token, &kit.wrapped) {
        Ok(()) => Ok(KitSaved::Saved),
        Err(ApiError::KeysExist) => {
            let wrapped = api.get_keys(token)?.ok_or(ApiError::Unexpected)?;
            let key = unwrap_account_key(derived.keys.wrap_key(), &wrapped.by_password)
                .map_err(|_| FlowError::DamagedKeys)?;
            Ok(KitSaved::AlreadySet(key, wrapped.by_password))
        }
        Err(err) => Err(err.into()),
    }
}

/// Proves the password on this device: derive with the stored salt and
/// Secret Key, then open the cached (or fetched) account key wrap.
pub fn unlock(
    kdf: AccountKdf,
    password: &SecretString,
    email: &str,
    salt: Salt,
    secret_key: AccountSecretKey,
    wrapped_by_password: &str,
) -> FlowResult<SecretKey> {
    let derived = derive(password, email, salt, secret_key, kdf)?;
    unwrap_account_key(derived.keys.wrap_key(), wrapped_by_password).map_err(|err| match err {
        CryptoError::Decrypt => FlowError::WrongPassword,
        _ => FlowError::DamagedKeys,
    })
}

/// The Secret Key text a user typed, canonical for storage.
pub fn parse_secret_key(text: &str) -> Result<(AccountSecretKey, Zeroizing<String>), String> {
    let key =
        AccountSecretKey::parse(text).map_err(|e| format!("That Secret Key isn't valid: {e}."))?;
    let shown = key.display();
    Ok((key, shown))
}
