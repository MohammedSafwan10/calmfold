//! Cryptographic primitives for the Keepstone vault.
//!
//! This crate wraps audited RustCrypto implementations. It does not invent
//! algorithms. Read `crates/crypto/AGENTS.md` before changing anything here.
//!
//! * Key derivation: Argon2id v1.3 ([`derive_key`]).
//! * Encryption: XChaCha20-Poly1305 with a random 24-byte nonce per message
//!   ([`seal`] / [`open`]), wrapped in a versioned envelope.
//! * All key material lives in types that zero their memory on drop and never
//!   print their contents.

mod envelope;
mod error;
mod kdf;
mod key;

pub use envelope::{ENVELOPE_OVERHEAD, open, seal};
pub use error::{CryptoError, CryptoResult};
pub use kdf::{KdfParams, Salt, derive_key};
pub use key::SecretKey;
pub use secrecy::{ExposeSecret, SecretString};

/// Fills `buf` with bytes from the operating system's CSPRNG.
pub fn random_bytes(buf: &mut [u8]) -> CryptoResult<()> {
    getrandom::fill(buf).map_err(|_| CryptoError::Random)
}
