//! The Passwords vault key wrapped by the Ottero Cloud account key
//! (ADR 0014, plan 06 §3).
//!
//! ```text
//! vault_link_key = HKDF-SHA256(ikm = account_key, salt = none, info = "ottero vault v1", 32)
//! wrapped        = seal(vault_link_key, vault_key, aad = "ottero vault key account v1")
//! ```
//!
//! `seal` is the vault's versioned envelope (`"KS" | 0x01 | nonce | ct | tag`,
//! [`crate::seal`]), the same format as the master-password and recovery
//! wrappings in `vault_meta`. The account key is never used directly as an
//! encryption key: HKDF with its own `info` separates this use from every
//! other key derived from it, and the AAD keeps this wrapping from being
//! confused with the vault's other wrappings (`keepstone-vault-key-v1`,
//! `keepstone-vault-recovery-v1`) or with the account key's own wraps.
//!
//! Changing either constant locks every linked vault; the fixed vectors in
//! the tests must never change.

use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::{CryptoError, CryptoResult, SecretKey, open, seal};

const INFO_VAULT: &[u8] = b"ottero vault v1";
const AAD_VAULT_KEY: &[u8] = b"ottero vault key account v1";

/// The key that wraps the vault key, derived from the account key.
fn vault_link_key(account_key: &SecretKey) -> CryptoResult<SecretKey> {
    let mut out = Zeroizing::new([0u8; SecretKey::LEN]);
    Hkdf::<Sha256>::new(None, account_key.expose())
        .expand(INFO_VAULT, &mut *out)
        .map_err(|_| CryptoError::Kdf)?;
    Ok(SecretKey::from_bytes(&mut out))
}

/// Seals `vault_key` so that `account_key` opens it.
pub fn wrap_vault_key(account_key: &SecretKey, vault_key: &SecretKey) -> CryptoResult<Vec<u8>> {
    seal(
        &vault_link_key(account_key)?,
        vault_key.expose(),
        AAD_VAULT_KEY,
    )
}

/// Opens a wrapping made by [`wrap_vault_key`]. Fails with
/// [`CryptoError::Decrypt`] for another account's key or a damaged value
/// (deliberately indistinguishable), and [`CryptoError::Format`] for
/// anything that isn't a 32-byte key in a version-1 envelope.
pub fn unwrap_vault_key(account_key: &SecretKey, wrapped: &[u8]) -> CryptoResult<SecretKey> {
    let bytes = open(&vault_link_key(account_key)?, wrapped, AAD_VAULT_KEY)?;
    if bytes.len() != SecretKey::LEN {
        return Err(CryptoError::Format);
    }
    let mut raw = Zeroizing::new([0u8; SecretKey::LEN]);
    raw.copy_from_slice(&bytes);
    Ok(SecretKey::from_bytes(&mut raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(start: u8) -> SecretKey {
        let mut raw = [0u8; SecretKey::LEN];
        for (i, b) in raw.iter_mut().enumerate() {
            *b = start.wrapping_add(i as u8);
        }
        SecretKey::from_bytes(&mut raw)
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// HKDF-SHA256(00 01 … 1f, no salt, "ottero vault v1"), checked with
    /// Python's `hmac`/`hashlib` (RFC 5869 extract + expand by hand).
    #[test]
    fn link_key_fixed_vector() {
        let k = vault_link_key(&key(0)).unwrap();
        assert_eq!(
            hex(k.expose()),
            "53671369525752297752b968490b7c1647383297f15abfe023ca6fbe163ae841"
        );
    }

    /// An envelope made with a fixed nonce (24 × 0x10) by an independent
    /// implementation (`@noble/ciphers` xchacha20poly1305 + `@noble/hashes`
    /// HKDF), sealing the vault key 0x40.. under account key 0x00..
    #[test]
    fn opens_a_fixed_vector() {
        let wrapped = unhex(
            "4b5301101010101010101010101010101010101010101010101010737d0618995cf1169e9cd1ff745a7610c37430ff6f9139cdc50ff8b6b90f14a6613779e1153642d39be558e43f730d93",
        );
        let opened = unwrap_vault_key(&key(0), &wrapped).unwrap();
        assert_eq!(opened.expose(), key(0x40).expose());
        assert_eq!(
            unwrap_vault_key(&key(1), &wrapped).map(|_| ()),
            Err(CryptoError::Decrypt),
            "another account's key"
        );
    }

    #[test]
    fn round_trip_with_fresh_nonces() {
        let (account, vault) = (key(7), SecretKey::generate().unwrap());
        let a = wrap_vault_key(&account, &vault).unwrap();
        let b = wrap_vault_key(&account, &vault).unwrap();
        assert_ne!(a, b, "a new nonce every time");
        assert_eq!(
            unwrap_vault_key(&account, &a).unwrap().expose(),
            vault.expose()
        );
        assert_eq!(
            unwrap_vault_key(&account, &b).unwrap().expose(),
            vault.expose()
        );
    }

    #[test]
    fn the_account_key_is_not_the_wrapping_key() {
        // Sealed directly with the account key (no HKDF), or with another
        // purpose's AAD, the wrapping must not open.
        let (account, vault) = (key(7), key(0x40));
        let direct = seal(&account, vault.expose(), AAD_VAULT_KEY).unwrap();
        assert_eq!(
            unwrap_vault_key(&account, &direct).map(|_| ()),
            Err(CryptoError::Decrypt)
        );
        let link = vault_link_key(&account).unwrap();
        for aad in [
            &b"keepstone-vault-key-v1"[..],
            b"keepstone-vault-recovery-v1",
            b"ottero account key v1",
            b"",
        ] {
            let other = seal(&link, vault.expose(), aad).unwrap();
            assert_eq!(
                unwrap_vault_key(&account, &other).map(|_| ()),
                Err(CryptoError::Decrypt),
                "AAD {aad:?}"
            );
        }
    }

    #[test]
    fn tampered_truncated_or_wrong_length_wrappings_are_rejected() {
        let (account, vault) = (key(7), key(0x40));
        let good = wrap_vault_key(&account, &vault).unwrap();
        for i in 0..good.len() {
            let mut bad = good.clone();
            bad[i] ^= 0x01;
            assert!(unwrap_vault_key(&account, &bad).is_err(), "byte {i}");
        }
        assert!(unwrap_vault_key(&account, &good[..good.len() - 1]).is_err());
        assert!(unwrap_vault_key(&account, &[]).is_err());
        // A well-formed envelope holding something other than a 32-byte key.
        let link = vault_link_key(&account).unwrap();
        let short = seal(&link, &[1; 16], AAD_VAULT_KEY).unwrap();
        assert_eq!(
            unwrap_vault_key(&account, &short).map(|_| ()),
            Err(CryptoError::Format)
        );
    }
}
