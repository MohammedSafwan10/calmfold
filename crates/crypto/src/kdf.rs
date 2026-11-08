use argon2::{Algorithm, Argon2, Params, Version};
use secrecy::{ExposeSecret, SecretString};

use crate::{CryptoError, CryptoResult, SecretKey, random_bytes};

/// Argon2id cost parameters. Stored next to the salt in the vault header so
/// they can be raised later without breaking existing vaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl KdfParams {
    /// Default for new vaults: RFC 9106 second recommended option
    /// (64 MiB, t=3, p=4). Well above the OWASP minimum.
    pub const RECOMMENDED: Self = Self {
        memory_kib: 64 * 1024,
        iterations: 3,
        parallelism: 4,
    };

    /// Lowest values we accept: OWASP's floor (19 MiB, t=2, p=1). Rejects
    /// weakened parameters from a damaged or tampered header.
    pub const MINIMUM: Self = Self {
        memory_kib: 19 * 1024,
        iterations: 2,
        parallelism: 1,
    };

    /// Upper bounds, so a malicious header can't make unlock exhaust memory or hang.
    const MAX_MEMORY_KIB: u32 = 1024 * 1024;
    const MAX_ITERATIONS: u32 = 64;
    const MAX_PARALLELISM: u32 = 16;

    /// Checks the parameters are within the allowed range.
    pub fn validate(&self) -> CryptoResult<()> {
        let ok = (Self::MINIMUM.memory_kib..=Self::MAX_MEMORY_KIB).contains(&self.memory_kib)
            && (Self::MINIMUM.iterations..=Self::MAX_ITERATIONS).contains(&self.iterations)
            && (Self::MINIMUM.parallelism..=Self::MAX_PARALLELISM).contains(&self.parallelism);
        if ok {
            Ok(())
        } else {
            Err(CryptoError::InvalidParams)
        }
    }
}

impl Default for KdfParams {
    fn default() -> Self {
        Self::RECOMMENDED
    }
}

/// A random 128-bit salt. Not secret, but must be unique per vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Salt(pub [u8; Salt::LEN]);

impl Salt {
    pub const LEN: usize = 16;

    pub fn generate() -> CryptoResult<Self> {
        let mut salt = [0; Self::LEN];
        random_bytes(&mut salt)?;
        Ok(Self(salt))
    }
}

/// Derives a 256-bit key from a password with Argon2id.
///
/// This is deliberately slow (around half a second with recommended
/// parameters). Never call it on the UI thread.
pub fn derive_key(
    password: &SecretString,
    salt: &Salt,
    params: KdfParams,
) -> CryptoResult<SecretKey> {
    params.validate()?;
    derive_key_unchecked(password, salt, params)
}

fn derive_key_unchecked(
    password: &SecretString,
    salt: &Salt,
    params: KdfParams,
) -> CryptoResult<SecretKey> {
    let argon_params = Params::new(
        params.memory_kib,
        params.iterations,
        params.parallelism,
        Some(SecretKey::LEN),
    )
    .map_err(|_| CryptoError::InvalidParams)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut key = SecretKey::zeroed();
    argon
        .hash_password_into(
            password.expose_secret().as_bytes(),
            &salt.0,
            key.bytes_mut(),
        )
        .map_err(|_| CryptoError::Kdf)?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cheap parameters so tests stay fast. Bypasses validation on purpose.
    const FAST: KdfParams = KdfParams {
        memory_kib: 64,
        iterations: 1,
        parallelism: 1,
    };

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    #[test]
    fn derivation_is_deterministic_per_password_and_salt() {
        let salt = Salt([1; Salt::LEN]);
        let a = derive_key_unchecked(&pw("correct horse"), &salt, FAST).expect("kdf");
        let b = derive_key_unchecked(&pw("correct horse"), &salt, FAST).expect("kdf");
        assert_eq!(a.expose(), b.expose());

        let other_pw = derive_key_unchecked(&pw("correct horsf"), &salt, FAST).expect("kdf");
        assert_ne!(a.expose(), other_pw.expose());

        let other_salt =
            derive_key_unchecked(&pw("correct horse"), &Salt([2; Salt::LEN]), FAST).expect("kdf");
        assert_ne!(a.expose(), other_salt.expose());
    }

    /// Pins the algorithm (Argon2id) and version (0x13): changing either would
    /// silently lock users out of existing vaults, so this must fail loudly.
    #[test]
    fn uses_argon2id_version_13() {
        let key =
            derive_key_unchecked(&pw("password"), &Salt(*b"somesaltsomesalt"), FAST).expect("kdf");
        let mut expected = [0u8; 32];
        Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            Params::new(64, 1, 1, Some(32)).expect("params"),
        )
        .hash_password_into(b"password", b"somesaltsomesalt", &mut expected)
        .expect("argon2");
        assert_eq!(key.expose(), &expected);
    }

    #[test]
    fn weak_or_absurd_params_are_rejected() {
        let salt = Salt([0; Salt::LEN]);
        assert_eq!(
            derive_key(&pw("x"), &salt, FAST).map(|_| ()),
            Err(CryptoError::InvalidParams)
        );
        let huge = KdfParams {
            memory_kib: u32::MAX,
            ..KdfParams::RECOMMENDED
        };
        assert_eq!(huge.validate(), Err(CryptoError::InvalidParams));
        let hung = KdfParams {
            iterations: 10_000,
            ..KdfParams::RECOMMENDED
        };
        assert_eq!(hung.validate(), Err(CryptoError::InvalidParams));
        assert_eq!(KdfParams::RECOMMENDED.validate(), Ok(()));
        assert_eq!(KdfParams::MINIMUM.validate(), Ok(()));
    }

    #[test]
    fn salts_are_random() {
        assert_ne!(
            Salt::generate().expect("salt"),
            Salt::generate().expect("salt")
        );
    }
}
