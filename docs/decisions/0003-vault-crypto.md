# 0003 Vault crypto: Argon2id + XChaCha20-Poly1305 (RustCrypto)

Date: 2026-11-08 · Status: accepted

Argon2id (RFC 9106 option 2 defaults, bounded params per vault) derives a key
that wraps a random vault key; items are sealed with XChaCha20-Poly1305
(192-bit random nonces, record ID as AAD) in a versioned envelope. RustCrypto
crates, AEAD audited by NCC Group. No custom primitives. Details:
`docs/security.md`, `crates/crypto/AGENTS.md`.
