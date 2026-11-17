//! Ottero Cloud account keys: one password plus a Secret Key (ADR 0010,
//! `docs/plans/06-sync.md` §2–3). The derivation is a wire format shared
//! with every future client, so the fixed vectors in the tests must never
//! change.
//!
//! ```text
//! password'  = NFKC(password)                        (UTF-8 bytes)
//! email'     = lowercase(trim(email))                 (UTF-8 bytes)
//! slow       = Argon2id v1.3(password', salt16, m=262144 KiB, t=3, p=1, 32 bytes)
//! mix        = HKDF-SHA256(ikm=secret_key16, salt=email', info="ottero secret key v1", 32)
//! master     = slow XOR mix
//! auth_key   = HKDF-SHA256(ikm=master, salt=none, info="ottero auth v1", 32)   → sent as hex
//! wrap_key   = HKDF-SHA256(ikm=master, salt=none, info="ottero wrap v1", 32)   → never sent
//! wrapped_by_password = base64url(nonce24 || XChaCha20-Poly1305(wrap_key, account_key,
//!                                   aad="ottero account key v1"))
//! recovery_wrap_key   = HKDF-SHA256(ikm=recovery_code32, salt=none, info="ottero recovery v1", 32)
//! wrapped_by_recovery = base64url(nonce24 || XChaCha20-Poly1305(recovery_wrap_key, account_key,
//!                                   aad="ottero account key recovery v1"))
//! ```
//!
//! The server sees the salt, `auth_key` (and hashes it again) and the two
//! wrapped keys. It never sees the password, the Secret Key, `wrap_key`,
//! the recovery code or `account_key`.
//!
//! Human formats use Crockford base32 (`0-9 A-Z` without `I L O U`), parsed
//! case-insensitively with spaces and dashes ignored and `O`→`0`, `I`/`L`→`1`:
//!
//! * Secret Key: `O1-XXXXXX-XXXXX-XXXXX-XXXXX-XXXXX`, version `O1` then 26
//!   characters holding 128 bits (two leading zero bits).
//! * Recovery code: 8 groups of 7 characters (56) holding the 32 random
//!   bytes plus a 3-byte checksum (first bytes of SHA-256 of the code), so a
//!   mistyped character is caught before any network call.

use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{AeadInOut, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization as _;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::{CryptoError, CryptoResult, KdfParams, Salt, SecretKey, random_bytes};

/// The only key derivation setting the account server accepts today.
pub const KDF_V1: &str = "argon2id:m=262144,t=3,p=1:v1";

const INFO_SECRET_KEY: &[u8] = b"ottero secret key v1";
const INFO_AUTH: &[u8] = b"ottero auth v1";
const INFO_WRAP: &[u8] = b"ottero wrap v1";
const INFO_RECOVERY: &[u8] = b"ottero recovery v1";
const AAD_PASSWORD: &[u8] = b"ottero account key v1";
const AAD_RECOVERY: &[u8] = b"ottero account key recovery v1";

const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
/// Bytes in a wrapped account key before base64url: nonce, key, tag.
const WRAPPED_LEN: usize = NONCE_LEN + SecretKey::LEN + TAG_LEN;

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Argon2id settings for an account, parsed from the server's `kdf` string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountKdf {
    params: KdfParams,
}

impl AccountKdf {
    /// `argon2id:m=262144,t=3,p=1:v1` (256 MiB, 3 passes, 1 lane).
    pub const V1: Self = Self {
        params: KdfParams {
            memory_kib: 262_144,
            iterations: 3,
            parallelism: 1,
        },
    };

    /// Cheap parameters so other crates' tests stay fast. Not available in
    /// shipped builds (feature `insecure-test-kdf`, dev-dependencies only).
    #[cfg(any(test, feature = "insecure-test-kdf"))]
    pub const INSECURE_TEST: Self = Self {
        params: KdfParams {
            memory_kib: 64,
            iterations: 1,
            parallelism: 1,
        },
    };

    /// Accepts exactly [`KDF_V1`]; anything else is refused so a server
    /// can't talk the app into weaker settings.
    pub fn parse(value: &str) -> CryptoResult<Self> {
        if value == KDF_V1 {
            Ok(Self::V1)
        } else {
            Err(CryptoError::InvalidParams)
        }
    }

    #[must_use]
    pub fn params(self) -> KdfParams {
        self.params
    }
}

/// A key or code in its human form couldn't be read. Messages are safe to
/// show: they never echo the key, only what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyFormatError {
    #[error("it has the wrong number of characters")]
    Length,
    #[error("it doesn't start with a supported version")]
    Version,
    #[error("it contains “{0}”, which isn't used")]
    Character(char),
    /// Valid characters that can't be a real key (a typo).
    #[error("it has a typo")]
    Checksum,
}

/// The 128-bit Secret Key, created on the device at sign-up and never sent
/// to the server. Zeroed on drop, redacted in `Debug`, not `Clone`.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct AccountSecretKey([u8; AccountSecretKey::LEN]);

impl AccountSecretKey {
    pub const LEN: usize = 16;
    const VERSION: &'static str = "O1";
    const CHARS: usize = 26;
    const GROUPS: [usize; 5] = [6, 5, 5, 5, 5];

    pub fn generate() -> CryptoResult<Self> {
        let mut key = Self([0; Self::LEN]);
        random_bytes(&mut key.0)?;
        Ok(key)
    }

    /// Takes ownership of raw bytes and zeroes the caller's copy.
    #[must_use]
    pub fn from_bytes(bytes: &mut [u8; Self::LEN]) -> Self {
        let key = Self(*bytes);
        bytes.zeroize();
        key
    }

    /// Reads `O1-XXXXXX-…`, tolerating case, spaces, dashes and the usual
    /// misreadings (`O`/`0`, `I`/`L`/`1`).
    pub fn parse(input: &str) -> Result<Self, KeyFormatError> {
        let canonical = canonical(input)?;
        if canonical.len() != Self::VERSION.len() + Self::CHARS {
            return Err(KeyFormatError::Length);
        }
        // `O1` canonicalizes to `01`.
        if &canonical[..2] != "01" {
            return Err(KeyFormatError::Version);
        }
        let mut bytes = Zeroizing::new([0u8; Self::LEN]);
        decode(&canonical.as_bytes()[2..], &mut *bytes)?;
        Ok(Self::from_bytes(&mut bytes))
    }

    /// `O1-XXXXXX-XXXXX-XXXXX-XXXXX-XXXXX`, for display and the Emergency Kit.
    #[must_use]
    pub fn display(&self) -> Zeroizing<String> {
        let body = encode(&self.0, Self::CHARS);
        let mut out = Zeroizing::new(String::with_capacity(40));
        out.push_str(Self::VERSION);
        let mut at = 0;
        for len in Self::GROUPS {
            out.push('-');
            out.push_str(&body[at..at + len]);
            at += len;
        }
        out
    }

    fn expose(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

impl fmt::Debug for AccountSecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountSecretKey([redacted])")
    }
}

/// The recovery code in the Emergency Kit: 32 random bytes that wrap the
/// account key a second time, for a forgotten password.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct RecoveryCode([u8; RecoveryCode::LEN]);

impl RecoveryCode {
    pub const LEN: usize = 32;
    const CHECKSUM: usize = 3;
    const GROUPS: usize = 8;
    const GROUP_LEN: usize = 7;

    pub fn generate() -> CryptoResult<Self> {
        let mut code = Self([0; Self::LEN]);
        random_bytes(&mut code.0)?;
        Ok(code)
    }

    #[must_use]
    pub fn from_bytes(bytes: &mut [u8; Self::LEN]) -> Self {
        let code = Self(*bytes);
        bytes.zeroize();
        code
    }

    /// Reads the 8 groups back, rejecting typos by the checksum.
    pub fn parse(input: &str) -> Result<Self, KeyFormatError> {
        let canonical = canonical(input)?;
        if canonical.len() != Self::GROUPS * Self::GROUP_LEN {
            return Err(KeyFormatError::Length);
        }
        let mut raw = Zeroizing::new([0u8; Self::LEN + Self::CHECKSUM]);
        decode(canonical.as_bytes(), &mut *raw)?;
        let (code, sum) = raw.split_at(Self::LEN);
        if Self::checksum(code) != sum {
            return Err(KeyFormatError::Checksum);
        }
        let mut bytes = Zeroizing::new([0u8; Self::LEN]);
        bytes.copy_from_slice(code);
        Ok(Self::from_bytes(&mut bytes))
    }

    /// `XXXXXXX-XXXXXXX-…` (8 groups of 7).
    #[must_use]
    pub fn display(&self) -> Zeroizing<String> {
        let mut raw = Zeroizing::new([0u8; Self::LEN + Self::CHECKSUM]);
        raw[..Self::LEN].copy_from_slice(&self.0);
        raw[Self::LEN..].copy_from_slice(&Self::checksum(&self.0));
        let body = encode(&*raw, Self::GROUPS * Self::GROUP_LEN);
        let mut out = Zeroizing::new(String::with_capacity(64));
        for (i, group) in body.as_bytes().chunks(Self::GROUP_LEN).enumerate() {
            if i > 0 {
                out.push('-');
            }
            out.push_str(std::str::from_utf8(group).unwrap_or_default());
        }
        out
    }

    fn checksum(code: &[u8]) -> [u8; Self::CHECKSUM] {
        let digest = Sha256::digest(code);
        [digest[0], digest[1], digest[2]]
    }
}

impl fmt::Debug for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryCode([redacted])")
    }
}

/// The login key sent to the server in place of the password.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct AuthKey([u8; 32]);

impl AuthKey {
    /// 64 lowercase hex characters, the server's expected `password` field.
    #[must_use]
    pub fn to_hex(&self) -> Zeroizing<String> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = Zeroizing::new(String::with_capacity(64));
        for b in self.0 {
            out.push(char::from(HEX[usize::from(b >> 4)]));
            out.push(char::from(HEX[usize::from(b & 15)]));
        }
        out
    }
}

impl fmt::Debug for AuthKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuthKey([redacted])")
    }
}

/// Both keys derived from the password and Secret Key.
pub struct AccountKeys {
    auth: AuthKey,
    wrap: SecretKey,
}

impl AccountKeys {
    #[must_use]
    pub fn auth_key(&self) -> &AuthKey {
        &self.auth
    }

    /// Wraps and unwraps the account key. Never leaves the device.
    #[must_use]
    pub fn wrap_key(&self) -> &SecretKey {
        &self.wrap
    }
}

impl fmt::Debug for AccountKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountKeys([redacted])")
    }
}

/// `trim` + lowercase, as the server normalizes it.
#[must_use]
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Derives the auth and wrap keys. Runs Argon2id with 256 MiB: about a
/// second, so never call it on the UI thread.
pub fn derive_account_keys(
    password: &SecretString,
    email: &str,
    salt: &Salt,
    secret_key: &AccountSecretKey,
    kdf: AccountKdf,
) -> CryptoResult<AccountKeys> {
    let params = kdf.params;
    let argon_params = Params::new(
        params.memory_kib,
        params.iterations,
        params.parallelism,
        Some(32),
    )
    .map_err(|_| CryptoError::InvalidParams)?;
    let password: Zeroizing<String> = Zeroizing::new(password.expose_secret().nfkc().collect());
    let mut master = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params)
        .hash_password_into(password.as_bytes(), &salt.0, &mut *master)
        .map_err(|_| CryptoError::Kdf)?;

    let email = normalize_email(email);
    let mut mix = Zeroizing::new([0u8; 32]);
    hkdf(
        secret_key.expose(),
        Some(email.as_bytes()),
        INFO_SECRET_KEY,
        &mut *mix,
    )?;
    for (m, x) in master.iter_mut().zip(mix.iter()) {
        *m ^= x;
    }

    let mut auth = AuthKey([0; 32]);
    hkdf(&*master, None, INFO_AUTH, &mut auth.0)?;
    let mut wrap = SecretKey::zeroed();
    hkdf(&*master, None, INFO_WRAP, wrap.bytes_mut())?;
    Ok(AccountKeys { auth, wrap })
}

/// The key that wraps the account key for recovery.
pub fn recovery_wrap_key(code: &RecoveryCode) -> CryptoResult<SecretKey> {
    let mut key = SecretKey::zeroed();
    hkdf(&code.0, None, INFO_RECOVERY, key.bytes_mut())?;
    Ok(key)
}

/// `wrapped_by_password`: the account key sealed by `wrap_key`.
pub fn wrap_account_key(wrap_key: &SecretKey, account_key: &SecretKey) -> CryptoResult<String> {
    seal(wrap_key, account_key, AAD_PASSWORD)
}

/// Opens `wrapped_by_password`. Fails with [`CryptoError::Decrypt`] for a
/// wrong password or Secret Key, or a damaged value.
pub fn unwrap_account_key(wrap_key: &SecretKey, wrapped: &str) -> CryptoResult<SecretKey> {
    open(wrap_key, wrapped, AAD_PASSWORD)
}

/// `wrapped_by_recovery`: the account key sealed by the recovery code.
pub fn wrap_account_key_for_recovery(
    code: &RecoveryCode,
    account_key: &SecretKey,
) -> CryptoResult<String> {
    seal(&recovery_wrap_key(code)?, account_key, AAD_RECOVERY)
}

/// Opens `wrapped_by_recovery` with the recovery code.
pub fn unwrap_account_key_with_recovery(
    code: &RecoveryCode,
    wrapped: &str,
) -> CryptoResult<SecretKey> {
    open(&recovery_wrap_key(code)?, wrapped, AAD_RECOVERY)
}

/// The 16-byte salt as base64url without padding (22 characters).
#[must_use]
pub fn encode_salt(salt: &Salt) -> String {
    URL_SAFE_NO_PAD.encode(salt.0)
}

/// Reads a salt from the server; anything but 22 base64url characters
/// for 16 bytes is refused.
pub fn decode_salt(value: &str) -> CryptoResult<Salt> {
    if value.len() != 22 {
        return Err(CryptoError::Format);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| CryptoError::Format)?;
    let bytes: [u8; Salt::LEN] = bytes.try_into().map_err(|_| CryptoError::Format)?;
    Ok(Salt(bytes))
}

fn hkdf(ikm: &[u8], salt: Option<&[u8]>, info: &[u8], out: &mut [u8]) -> CryptoResult<()> {
    Hkdf::<Sha256>::new(salt, ikm)
        .expand(info, out)
        .map_err(|_| CryptoError::Kdf)
}

fn seal(key: &SecretKey, account_key: &SecretKey, aad: &[u8]) -> CryptoResult<String> {
    let mut nonce = [0u8; NONCE_LEN];
    random_bytes(&mut nonce)?;
    let mut buffer = Zeroizing::new(Vec::with_capacity(SecretKey::LEN + TAG_LEN));
    buffer.extend_from_slice(account_key.expose());
    XChaCha20Poly1305::new_from_slice(key.expose())
        .map_err(|_| CryptoError::Encrypt)?
        .encrypt_in_place(&XNonce::from(nonce), aad, &mut *buffer)
        .map_err(|_| CryptoError::Encrypt)?;
    let mut out = Vec::with_capacity(WRAPPED_LEN);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&buffer);
    Ok(URL_SAFE_NO_PAD.encode(out))
}

fn open(key: &SecretKey, wrapped: &str, aad: &[u8]) -> CryptoResult<SecretKey> {
    let raw = URL_SAFE_NO_PAD
        .decode(wrapped.trim())
        .map_err(|_| CryptoError::Format)?;
    if raw.len() != WRAPPED_LEN {
        return Err(CryptoError::Format);
    }
    let (nonce, ciphertext) = raw.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| CryptoError::Format)?;
    let mut buffer = Zeroizing::new(ciphertext.to_vec());
    XChaCha20Poly1305::new_from_slice(key.expose())
        .map_err(|_| CryptoError::Decrypt)?
        .decrypt_in_place(&XNonce::from(nonce), aad, &mut *buffer)
        .map_err(|_| CryptoError::Decrypt)?;
    let mut bytes = Zeroizing::new([0u8; SecretKey::LEN]);
    bytes.copy_from_slice(&buffer);
    Ok(SecretKey::from_bytes(&mut bytes))
}

/// Upper case without separators, misreadings fixed, Crockford characters only.
fn canonical(input: &str) -> Result<Zeroizing<String>, KeyFormatError> {
    let mut out = Zeroizing::new(String::with_capacity(64));
    for c in input.chars() {
        if c.is_whitespace() || matches!(c, '-' | '‐' | '–' | '—') {
            continue;
        }
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        if !c.is_ascii() || !CROCKFORD.contains(&(c as u8)) {
            return Err(KeyFormatError::Character(c));
        }
        out.push(c);
    }
    Ok(out)
}

/// MSB-first base32 of `bytes` in exactly `chars` characters; the stream
/// is left-padded with zero bits to a multiple of five.
fn encode(bytes: &[u8], chars: usize) -> Zeroizing<String> {
    let pad = chars * 5 - bytes.len() * 8;
    let bit = |k: usize| -> u8 {
        if k < pad {
            0
        } else {
            let k = k - pad;
            (bytes[k / 8] >> (7 - k % 8)) & 1
        }
    };
    let mut out = Zeroizing::new(String::with_capacity(chars));
    for i in 0..chars {
        let mut v = 0u8;
        for j in 0..5 {
            v = (v << 1) | bit(i * 5 + j);
        }
        out.push(char::from(CROCKFORD[usize::from(v)]));
    }
    out
}

/// The inverse of [`encode`]. `text` is already canonical. Padding bits
/// must be zero, otherwise the text can't have come from `encode`.
fn decode(text: &[u8], out: &mut [u8]) -> Result<(), KeyFormatError> {
    let pad = text.len() * 5 - out.len() * 8;
    out.fill(0);
    for (i, c) in text.iter().enumerate() {
        let v = CROCKFORD
            .iter()
            .position(|a| a == c)
            .ok_or(KeyFormatError::Character(char::from(*c)))?;
        for j in 0..5 {
            let k = i * 5 + j;
            let b = ((v >> (4 - j)) & 1) as u8;
            if k < pad {
                if b != 0 {
                    return Err(KeyFormatError::Checksum);
                }
            } else {
                let k = k - pad;
                out[k / 8] |= b << (7 - k % 8);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FAST: AccountKdf = AccountKdf::INSECURE_TEST;

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    fn fixed_secret() -> AccountSecretKey {
        let mut b: [u8; 16] = std::array::from_fn(|i| i as u8);
        AccountSecretKey::from_bytes(&mut b)
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn keys(password: &str, email: &str, secret: &AccountSecretKey) -> AccountKeys {
        derive_account_keys(&pw(password), email, &Salt([7; 16]), secret, FAST).unwrap()
    }

    /// RFC 5869 test case 1: proves the HKDF-SHA256 wiring.
    #[test]
    fn hkdf_matches_rfc_5869_case_1() {
        let ikm = [0x0b; 22];
        let salt: Vec<u8> = (0x00..=0x0c).collect();
        let info: Vec<u8> = (0xf0..=0xf9).collect();
        let mut okm = [0u8; 42];
        hkdf(&ikm, Some(&salt), &info, &mut okm).unwrap();
        assert_eq!(
            hex(&okm),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
        );
    }

    /// Fixed inputs → fixed keys. Changing any step of the derivation
    /// would lock every account out, so this must fail loudly.
    #[test]
    fn derivation_vector_with_test_params() {
        let k = keys(
            "correct horse battery staple",
            " Sam@Example.com ",
            &fixed_secret(),
        );
        assert_eq!(
            k.auth_key().to_hex().as_str(),
            "9807ca227759618a82bcf216169c3f6e5855d7fe572fdba1939747d177e4b536"
        );
        assert_eq!(
            hex(k.wrap_key().expose()),
            "5834ea95f7a407943840d14008640faaaf03e1581e4f5d4e96d54876a82afed6"
        );
    }

    /// The real 256 MiB parameters (about a second). Vectors in this module
    /// were cross-checked against Node's `crypto.hkdfSync` + `@noble/hashes` Argon2id.
    #[test]
    fn derivation_vector_with_real_params() {
        let k = derive_account_keys(
            &pw("correct horse battery staple"),
            "sam@example.com",
            &Salt([7; 16]),
            &fixed_secret(),
            AccountKdf::V1,
        )
        .unwrap();
        assert_eq!(
            k.auth_key().to_hex().as_str(),
            "546857dca519180e8a0df498bd5ed71cc049c8bfaadc300ca3703a94be48f7f9"
        );
        assert_eq!(
            hex(k.wrap_key().expose()),
            "4c19a874f30d4e0fe29ec12ef4d4181169a4fb26fadfaeb85fc9b9595f7ad8d2"
        );
    }

    #[test]
    fn auth_and_wrap_keys_differ_and_hex_is_lowercase() {
        let k = keys("pw-123456789", "a@b.co", &fixed_secret());
        let hex_key = k.auth_key().to_hex();
        assert_eq!(hex_key.len(), 64);
        assert!(
            hex_key
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
        assert_ne!(hex_key.as_str(), hex(k.wrap_key().expose()));
    }

    #[test]
    fn every_input_changes_the_keys() {
        let base = keys("pw-123456789", "a@b.co", &fixed_secret());
        let other_pw = keys("pw-123456780", "a@b.co", &fixed_secret());
        let other_email = keys("pw-123456789", "c@b.co", &fixed_secret());
        let other_secret = keys(
            "pw-123456789",
            "a@b.co",
            &AccountSecretKey::from_bytes(&mut [9; 16]),
        );
        let other_salt = derive_account_keys(
            &pw("pw-123456789"),
            "a@b.co",
            &Salt([8; 16]),
            &fixed_secret(),
            FAST,
        )
        .unwrap();
        for k in [other_pw, other_email, other_secret, other_salt] {
            assert_ne!(base.auth_key().to_hex(), k.auth_key().to_hex());
            assert_ne!(base.wrap_key().expose(), k.wrap_key().expose());
        }
    }

    #[test]
    fn email_is_trimmed_and_case_folded_and_password_is_nfkc() {
        let a = keys("pw-123456789", "Sam@Example.COM", &fixed_secret());
        let b = keys("pw-123456789", "  sam@example.com\n", &fixed_secret());
        assert_eq!(a.auth_key().to_hex(), b.auth_key().to_hex());
        // "ﬁ" (U+FB01) is NFKC-equal to "fi"; full-width digits to ASCII.
        let c = keys("ﬁsh-１２３４５６", "a@b.co", &fixed_secret());
        let d = keys("fish-123456", "a@b.co", &fixed_secret());
        assert_eq!(c.auth_key().to_hex(), d.auth_key().to_hex());
    }

    #[test]
    fn wrap_round_trip_and_wrong_keys_fail() {
        let account = SecretKey::generate().unwrap();
        let right = keys("pw-123456789", "a@b.co", &fixed_secret());
        let wrapped = wrap_account_key(right.wrap_key(), &account).unwrap();
        assert!((40..=700).contains(&wrapped.len()), "server accepts it");
        assert!(
            wrapped
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
        let opened = unwrap_account_key(right.wrap_key(), &wrapped).unwrap();
        assert_eq!(opened.expose(), account.expose());

        let wrong_pw = keys("pw-123456780", "a@b.co", &fixed_secret());
        let wrong_secret = keys(
            "pw-123456789",
            "a@b.co",
            &AccountSecretKey::from_bytes(&mut [1; 16]),
        );
        for k in [wrong_pw, wrong_secret] {
            assert_eq!(
                unwrap_account_key(k.wrap_key(), &wrapped).map(|_| ()),
                Err(CryptoError::Decrypt)
            );
        }
    }

    #[test]
    fn tampered_or_malformed_wraps_are_rejected() {
        let account = SecretKey::generate().unwrap();
        let k = keys("pw-123456789", "a@b.co", &fixed_secret());
        let wrapped = wrap_account_key(k.wrap_key(), &account).unwrap();
        let raw = URL_SAFE_NO_PAD.decode(&wrapped).unwrap();
        for i in 0..raw.len() {
            let mut damaged = raw.clone();
            damaged[i] ^= 0x01;
            assert_eq!(
                unwrap_account_key(k.wrap_key(), &URL_SAFE_NO_PAD.encode(&damaged)).map(|_| ()),
                Err(CryptoError::Decrypt),
                "byte {i}"
            );
        }
        for bad in ["", "not base64!", &wrapped[..wrapped.len() - 4]] {
            assert_eq!(
                unwrap_account_key(k.wrap_key(), bad).map(|_| ()),
                Err(CryptoError::Format)
            );
        }
        // The password wrap can't be opened as a recovery wrap (AAD differs).
        let code = RecoveryCode::generate().unwrap();
        assert!(unwrap_account_key_with_recovery(&code, &wrapped).is_err());
    }

    #[test]
    fn recovery_wrap_round_trip() {
        let account = SecretKey::generate().unwrap();
        let code = RecoveryCode::generate().unwrap();
        let wrapped = wrap_account_key_for_recovery(&code, &account).unwrap();
        let typed = RecoveryCode::parse(&code.display().to_lowercase()).unwrap();
        let opened = unwrap_account_key_with_recovery(&typed, &wrapped).unwrap();
        assert_eq!(opened.expose(), account.expose());
        let other = RecoveryCode::generate().unwrap();
        assert_eq!(
            unwrap_account_key_with_recovery(&other, &wrapped).map(|_| ()),
            Err(CryptoError::Decrypt)
        );
        // Same AAD separation the other way round.
        let wrap = recovery_wrap_key(&code).unwrap();
        assert!(unwrap_account_key(&wrap, &wrapped).is_err());
    }

    #[test]
    fn recovery_wrap_key_vector() {
        let code = RecoveryCode::from_bytes(&mut [0x42; 32]);
        assert_eq!(
            hex(recovery_wrap_key(&code).unwrap().expose()),
            "71ba11092bd9bedeafd7a038e8524a7d38ad139c3f1630bb6716945096ef5615"
        );
    }

    #[test]
    fn secret_key_format_vector() {
        let key = fixed_secret();
        let shown = key.display();
        assert_eq!(shown.as_str(), "O1-000410-61050-R3GG2-8A1C6-0T3GF");
        let groups: Vec<_> = shown.split('-').map(str::len).collect();
        assert_eq!(groups, [2, 6, 5, 5, 5, 5]);
    }

    #[test]
    fn secret_key_parse_is_tolerant() {
        let key = fixed_secret();
        let shown = key.display().to_string();
        let variants = [
            shown.clone(),
            shown.to_lowercase(),
            shown.replace('-', " "),
            shown.replace('-', ""),
            format!("  {shown}\n"),
            shown.replace('1', "I").replacen("0", "O", 3),
            shown.to_lowercase().replace('1', "l"),
        ];
        for v in variants {
            let parsed = AccountSecretKey::parse(&v).unwrap_or_else(|e| panic!("{v}: {e}"));
            assert_eq!(parsed.expose(), key.expose(), "{v}");
        }
    }

    #[test]
    fn secret_key_parse_rejects_bad_input() {
        let shown = fixed_secret().display().to_string();
        assert_eq!(
            AccountSecretKey::parse(&shown[..shown.len() - 1]).map(|_| ()),
            Err(KeyFormatError::Length)
        );
        assert_eq!(
            AccountSecretKey::parse(&format!("{shown}0")).map(|_| ()),
            Err(KeyFormatError::Length)
        );
        assert_eq!(
            AccountSecretKey::parse(&shown.replacen("O1", "O2", 1)).map(|_| ()),
            Err(KeyFormatError::Version)
        );
        assert_eq!(
            AccountSecretKey::parse(&shown.replacen('0', "U", 1)).map(|_| ()),
            Err(KeyFormatError::Character('U'))
        );
        assert_eq!(
            AccountSecretKey::parse("O1-!").map(|_| ()),
            Err(KeyFormatError::Character('!'))
        );
        // First payload character carries two padding bits: 8+ can't occur.
        let mut too_big = shown.clone();
        too_big.replace_range(3..4, "Z");
        assert_eq!(
            AccountSecretKey::parse(&too_big).map(|_| ()),
            Err(KeyFormatError::Checksum)
        );
        assert_eq!(
            AccountSecretKey::parse("").map(|_| ()),
            Err(KeyFormatError::Length)
        );
    }

    /// Property: every random key survives display → parse, in any typing style.
    #[test]
    fn secret_keys_round_trip_property() {
        for i in 0..500 {
            let key = AccountSecretKey::generate().unwrap();
            let shown = key.display();
            assert!(shown.starts_with("O1-"));
            assert_eq!(shown.len(), 33);
            let typed = if i % 2 == 0 {
                shown.to_lowercase()
            } else {
                shown.replace('-', " ")
            };
            assert_eq!(
                AccountSecretKey::parse(&typed).unwrap().expose(),
                key.expose()
            );
        }
    }

    /// Property: recovery codes round trip, and any one changed character is caught.
    #[test]
    fn recovery_codes_round_trip_and_catch_typos() {
        for _ in 0..200 {
            let code = RecoveryCode::generate().unwrap();
            let shown = code.display();
            assert_eq!(shown.split('-').count(), 8);
            assert!(shown.split('-').all(|g| g.len() == 7));
            assert_eq!(RecoveryCode::parse(&shown).unwrap().0, code.0);

            let mut chars: Vec<char> = shown.chars().collect();
            let at = chars.iter().position(|c| *c != '-').unwrap_or(0) + 9;
            let at = if chars[at] == '-' { at + 1 } else { at };
            chars[at] = if chars[at] == 'A' { 'B' } else { 'A' };
            let typo: String = chars.into_iter().collect();
            assert!(RecoveryCode::parse(&typo).is_err(), "typo accepted");
        }
        assert_eq!(
            RecoveryCode::parse("ABC").map(|_| ()),
            Err(KeyFormatError::Length)
        );
    }

    #[test]
    fn kdf_string_is_exact_and_salts_round_trip() {
        assert_eq!(AccountKdf::parse(KDF_V1), Ok(AccountKdf::V1));
        for bad in [
            "",
            "argon2id:m=65536,t=3,p=1:v1",
            "pbkdf2",
            "ARGON2ID:m=262144,t=3,p=1:v1",
        ] {
            assert_eq!(AccountKdf::parse(bad), Err(CryptoError::InvalidParams));
        }
        let salt = Salt::generate().unwrap();
        let text = encode_salt(&salt);
        assert_eq!(text.len(), 22);
        assert_eq!(decode_salt(&text), Ok(salt));
        assert_eq!(decode_salt("short"), Err(CryptoError::Format));
        assert_eq!(decode_salt(&"*".repeat(22)), Err(CryptoError::Format));
    }

    #[test]
    fn secrets_never_print() {
        let k = keys("pw-123456789", "a@b.co", &fixed_secret());
        assert_eq!(format!("{k:?}"), "AccountKeys([redacted])");
        assert_eq!(format!("{:?}", k.auth_key()), "AuthKey([redacted])");
        assert_eq!(
            format!("{:?}", fixed_secret()),
            "AccountSecretKey([redacted])"
        );
        let code = RecoveryCode::generate().unwrap();
        assert_eq!(format!("{code:?}"), "RecoveryCode([redacted])");
    }
}
