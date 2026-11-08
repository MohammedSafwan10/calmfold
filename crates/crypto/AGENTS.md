# keepstone-crypto

Key derivation and authenticated encryption for the password vault. Mistakes
here lose user data or leak it, so the rules are strict.

## Rules

- Use only audited RustCrypto primitives already in `Cargo.toml`
  (`argon2`, `chacha20poly1305`, `zeroize`, `secrecy`, `getrandom`). Adding a
  crypto dependency needs an ADR in `docs/decisions/`.
- Never write a cipher, MAC, KDF, or RNG yourself.
- Key material lives in `SecretKey` / `SecretString`: zeroed on drop,
  redacted in `Debug`, not `Clone`. Never log, format, or serialize it.
- Errors never reveal *why* decryption failed (wrong key vs tampering).
- Every new nonce comes from `random_bytes`. Never reuse or derive nonces.
- The envelope format (`envelope.rs`) is versioned. To change it, add a new
  version byte and keep decrypting old versions. Never change version 1.
- `KdfParams` are stored per vault. Raising defaults must not break old vaults.
- Argon2 is slow on purpose; never call `derive_key` on the UI thread.

## Format (v1)

`"KS" | 0x01 | nonce[24] | ciphertext | tag[16]`, with header + length-prefixed
caller AAD as associated data. Callers pass the record ID as AAD so a
ciphertext can't be moved to another record.

## Test

`cargo test -p keepstone-crypto`. Tamper, wrong-key, wrong-AAD, truncation and
parameter-bound tests must keep passing.
