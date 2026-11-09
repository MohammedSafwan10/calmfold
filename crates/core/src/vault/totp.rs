//! One-time passwords (RFC 6238) via `totp-rs` (ADR 0005).
//!
//! Accepts an `otpauth://totp/...` link (what QR codes contain) or a bare
//! base32 setup key as most sites show it ("JBSW Y3DP EHPK 3PXP"). Short
//! secrets that real sites still use (80 bits) are accepted.

use totp_rs::{Builder, Secret, Totp};

use crate::error::{DomainError, DomainResult};

/// A parsed setup key. Holds the secret; zeroed on drop by `totp-rs`.
pub struct OneTime(Totp);

/// The current code and how long it stays valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    pub code: String,
    pub seconds_left: u64,
    pub period: u64,
}

impl Code {
    /// "123 456" for six digits, "1234 5678" for eight.
    #[must_use]
    pub fn grouped(&self) -> String {
        let mid = self.code.len() / 2;
        format!("{} {}", &self.code[..mid], &self.code[mid..])
    }
}

pub fn parse(input: &str) -> DomainResult<OneTime> {
    let input = input.trim();
    if input.to_lowercase().starts_with("otpauth://") {
        return Totp::from_url_unchecked(input)
            .map(OneTime)
            .map_err(|_| DomainError::InvalidOneTimeCode);
    }
    let compact: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>()
        .to_uppercase();
    if compact.len() < 16 {
        return Err(DomainError::InvalidOneTimeCode);
    }
    let secret = Secret::try_from_base32(compact.trim_end_matches('='))
        .map_err(|_| DomainError::InvalidOneTimeCode)?;
    Ok(OneTime(
        Builder::new().with_secret(secret).build_noncompliant(),
    ))
}

impl OneTime {
    /// The code at `unix_seconds`.
    #[must_use]
    pub fn code_at(&self, unix_seconds: u64) -> Code {
        let period = self.0.step().max(1);
        Code {
            code: self.0.generate(unix_seconds).to_string(),
            seconds_left: period - unix_seconds % period,
            period,
        }
    }

    /// The issuer named in an otpauth link ("GitHub"), if any.
    #[must_use]
    pub fn issuer(&self) -> Option<String> {
        self.0.issuer().map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 6238 test secret "12345678901234567890" in base32.
    const RFC: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    #[test]
    fn matches_the_rfc_test_vector() {
        let otp = parse(RFC).expect("parse");
        // RFC 6238 Appendix B, SHA-1, T = 59 → 94287082 (8 digits); 6 digits → 287082.
        let code = otp.code_at(59);
        assert_eq!(code.code, "287082");
        assert_eq!(code.seconds_left, 1);
        assert_eq!(code.grouped(), "287 082");
    }

    #[test]
    fn accepts_links_spaced_keys_and_rejects_junk() {
        let link = format!("otpauth://totp/GitHub:sam?secret={RFC}&issuer=GitHub");
        let otp = parse(&link).expect("link");
        assert_eq!(otp.issuer().as_deref(), Some("GitHub"));
        assert_eq!(otp.code_at(59).code, "287082");
        assert!(
            parse("gezd gnbv gy3t qojq gezd gnbv gy3t qojq").is_ok(),
            "spaced, lower case"
        );
        assert!(parse("hello").is_err());
        assert!(parse("!!!!!!!!!!!!!!!!!!!!").is_err());
        assert!(parse("otpauth://hotp/x?secret=ABC").is_err());
    }
}
