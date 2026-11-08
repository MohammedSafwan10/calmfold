use std::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{CryptoResult, random_bytes};

/// A 256-bit symmetric key. Zeroed on drop, redacted in `Debug`, not `Clone`.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SecretKey([u8; SecretKey::LEN]);

impl SecretKey {
    pub const LEN: usize = 32;

    /// A fresh random key from the OS CSPRNG.
    pub fn generate() -> CryptoResult<Self> {
        let mut key = Self([0; Self::LEN]);
        random_bytes(&mut key.0)?;
        Ok(key)
    }

    /// Takes ownership of raw key bytes and zeroes the caller's copy.
    #[must_use]
    pub fn from_bytes(bytes: &mut [u8; Self::LEN]) -> Self {
        let key = Self(*bytes);
        bytes.zeroize();
        key
    }

    /// Raw bytes. Keep the borrow short and never copy them into a long-lived buffer.
    #[must_use]
    pub fn expose(&self) -> &[u8; Self::LEN] {
        &self.0
    }

    pub(crate) fn zeroed() -> Self {
        Self([0; Self::LEN])
    }

    pub(crate) fn bytes_mut(&mut self) -> &mut [u8; Self::LEN] {
        &mut self.0
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKey([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_is_redacted() {
        let key = SecretKey::generate().expect("key");
        assert_eq!(format!("{key:?}"), "SecretKey([redacted])");
    }

    #[test]
    fn generated_keys_differ() {
        let a = SecretKey::generate().expect("key");
        let b = SecretKey::generate().expect("key");
        assert_ne!(a.expose(), b.expose());
        assert_ne!(a.expose(), &[0; SecretKey::LEN]);
    }

    #[test]
    fn from_bytes_wipes_the_source() {
        let mut raw = [7u8; SecretKey::LEN];
        let key = SecretKey::from_bytes(&mut raw);
        assert_eq!(raw, [0; SecretKey::LEN]);
        assert_eq!(key.expose(), &[7; SecretKey::LEN]);
    }
}
