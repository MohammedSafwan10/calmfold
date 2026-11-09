//! Recovery keys: a second way into the vault for a forgotten master password.
//!
//! A recovery key is 160 random bits written as 32 base32 characters in
//! eight groups (`ABCD-EFGH-…`), shown once at setup and saved by the user
//! in an Emergency Kit outside Keepstone. The app derives a second key from
//! it that also wraps the vault key, so either the master password or the
//! recovery key opens the vault (ADR 0006). Only the user ever holds it.

use zeroize::Zeroizing;

/// Random bytes in a recovery key.
pub const KEY_BYTES: usize = 20;
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// `ABCD-EFGH-…` from raw bytes (base32, RFC 4648, no padding).
#[must_use]
pub fn format(bytes: &[u8; KEY_BYTES]) -> Zeroizing<String> {
    let mut chars = Zeroizing::new(String::with_capacity(40));
    let mut buffer: u32 = 0;
    let mut bits = 0;
    let mut written = 0;
    for &b in bytes {
        buffer = (buffer << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            if written > 0 && written % 4 == 0 {
                chars.push('-');
            }
            chars.push(char::from(ALPHABET[((buffer >> bits) & 31) as usize]));
            written += 1;
        }
    }
    chars
}

/// The canonical form of what the user typed: upper case, no separators or
/// spaces, and the usual misreadings fixed (`0`→`O`, `1`→`I`, `8`→`B`).
/// `None` unless it is exactly a 32-character key.
#[must_use]
pub fn normalize(input: &str) -> Option<Zeroizing<String>> {
    let mut out = Zeroizing::new(String::with_capacity(32));
    for c in input.chars() {
        let c = c.to_ascii_uppercase();
        let c = match c {
            '0' => 'O',
            '1' => 'I',
            '8' => 'B',
            c => c,
        };
        if c.is_whitespace() || c == '-' {
            continue;
        }
        if !ALPHABET.contains(&(c as u8)) || !c.is_ascii() {
            return None;
        }
        out.push(c);
    }
    (out.len() == 32).then_some(out)
}

/// Plain-text Emergency Kit. Holds the recovery key, never the master password.
#[must_use]
pub fn emergency_kit(key: &str, created: &str) -> Zeroizing<String> {
    Zeroizing::new(format!(
        "KEEPSTONE EMERGENCY KIT\n\
         Created {created}\n\n\
         Recovery key\n\
         {key}\n\n\
         What this is\n\
         If you forget your master password, open Keepstone → Passwords →\n\
         \"Forgot master password?\" → Use recovery key, and type this key.\n\
         You'll then choose a new master password.\n\n\
         Keep it safe\n\
         Anyone with this key can open your vault. Print it and keep it with\n\
         your important papers, or store it in a place only you can open.\n\
         Don't keep it on the same computer, and don't email it to yourself.\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_normalizes_round_trip() {
        let bytes = [0xA5u8; KEY_BYTES];
        let key = format(&bytes);
        assert_eq!(key.len(), 32 + 7, "eight groups of four");
        assert_eq!(key.split('-').count(), 8);
        let typed = key.to_lowercase().replace('-', " ");
        assert_eq!(
            normalize(&typed).map(|n| n.to_string()),
            Some(key.replace("-", ""))
        );
    }

    #[test]
    fn known_vector_and_misreadings() {
        // RFC 4648: "foobar" → MZXW6YTBOI; pad to 20 bytes with zeros.
        let mut bytes = [0u8; KEY_BYTES];
        bytes[..6].copy_from_slice(b"foobar");
        assert!(format(&bytes).starts_with("MZXW-6YTB-OI"));
        let k = "AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAA0";
        assert!(
            normalize(k).is_some_and(|n| n.ends_with('O')),
            "0 read as O"
        );
        assert!(normalize("too short").is_none());
        assert!(normalize("AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAA!").is_none());
    }

    #[test]
    fn the_kit_never_mentions_a_master_password_value() {
        let kit = emergency_kit("ABCD-EFGH", "Nov 9, 2026");
        assert!(kit.contains("ABCD-EFGH"));
        assert!(kit.contains("Anyone with this key"));
    }
}
