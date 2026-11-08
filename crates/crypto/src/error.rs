use thiserror::Error;

/// Errors never include key material, plaintext, or which check failed during
/// decryption, so they are safe to log and do not help an attacker.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CryptoError {
    #[error("the system random number generator failed")]
    Random,
    #[error("key derivation parameters are outside the allowed range")]
    InvalidParams,
    #[error("key derivation failed")]
    Kdf,
    #[error("encryption failed")]
    Encrypt,
    /// Wrong key, wrong associated data, or tampered ciphertext. Deliberately
    /// indistinguishable.
    #[error("decryption failed: wrong password or damaged data")]
    Decrypt,
    #[error("encrypted data has an unknown format or version")]
    Format,
}

pub type CryptoResult<T> = Result<T, CryptoError>;
