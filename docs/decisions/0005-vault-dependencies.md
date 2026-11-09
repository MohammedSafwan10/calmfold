# 0005 Password vault: format and dependencies

Status: accepted (2026-11-09)

## Context

The vault needs one-time codes (TOTP), a password-strength estimate, a
passphrase word list and CSV import from other managers. `crates/crypto/AGENTS.md`
requires an ADR before adding anything crypto-adjacent.

## Decision

- **Key hierarchy.** Master password → Argon2id (`KdfParams::RECOMMENDED`,
  stored per vault) → *key-encryption key* → unwraps a random 256-bit *vault
  key* (`seal(kek, vault_key, aad = "keepstone-vault-key-v1")`). Items are
  `seal(vault_key, json, aad = item id)`. Changing the master password only
  re-wraps the vault key. A wrong password is detected by the AEAD tag on the
  wrapped key, with no separate verifier.
- **Everything about an item is encrypted** (title, URLs, kind, tags,
  favorite). Only the item ID and `updated_at`/`deleted_at` are plaintext.
  Search runs over the decrypted in-memory list; nothing secret is indexed.
- **`totp-rs` 6** (MIT; RustCrypto `hmac`/`sha1`/`sha2`; `otpauth` feature for
  `otpauth://` links). HMAC is the RFC 6238 primitive; we don't write it.
- **`zxcvbn` 3** (MIT, Dropbox's estimator) for the strength meter and
  password health. Runs locally; no network, no breach lookups.
- **EFF large word list** (7,776 words, CC BY 3.0 US, `crates/core/data/`)
  for passphrases. 12.9 bits per word.
- **`csv` 1** for importing Bitwarden / 1Password / Chrome exports.
- **Random choice** uses `random_bytes` with rejection sampling
  (`keepstone_crypto::random_below`), so every character and word is uniform.

## Not now

- Encrypted export (`age`) and a secret key: with sync. (A password-only
  `.ksvault` backup exists since 2026-11-09; see `docs/security.md`.)
- Touch ID quick unlock: release builds only (signing policy in `docs/security.md`).

## Consequences

Losing the master password loses the vault; setup says so and asks for
acknowledgement. Unlock costs one Argon2 run (~0.3–1 s), done off the UI thread.
