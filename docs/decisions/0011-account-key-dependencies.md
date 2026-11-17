# 0011 Ottero Cloud account keys: format and dependencies

Status: accepted (2026-11-17)

## Context

Signing up and signing in to Ottero Cloud (plan 06 §2–3, ADR 0010) derive an
auth key and a wrap key on the device from the password and a Secret Key.
`crates/crypto/AGENTS.md` requires an ADR before adding a crypto dependency.
The derivation is a wire format: every future client (mobile, web) must
produce the same bytes, so it is written down here and pinned by fixed
vectors in `crates/crypto/src/account.rs`.

## Decision

- **Derivation** (`ottero_crypto::account`):
  `master = Argon2id(NFKC(password), salt16, m=262144 KiB, t=3, p=1, 32)`
  XOR `HKDF-SHA256(ikm=secret_key16, salt=lowercase(trim(email)), info="ottero secret key v1", 32)`;
  `auth_key = HKDF(master, info="ottero auth v1")` (sent as 64 lowercase hex);
  `wrap_key = HKDF(master, info="ottero wrap v1")` (never sent).
  The server's `kdf` string must equal `argon2id:m=262144,t=3,p=1:v1`;
  anything else is refused so a server can't downgrade the parameters.
- **Account key**: 32 random bytes, wrapped as
  `base64url(nonce24 || XChaCha20-Poly1305(wrap_key, account_key, aad="ottero account key v1"))`.
  This is a bare AEAD, not the vault envelope (`"KS"|0x01|…`), because the
  server validates the field length and other clients must parse it without
  Ottero's envelope.
- **Secret Key** text: `O1-` + 26 Crockford base32 characters (128 bits,
  two leading zero bits), grouped 6/5/5/5/5. Parsing ignores case, spaces and
  dashes and reads `O` as `0`, `I`/`L` as `1`; nonzero padding bits are
  rejected as a typo.
- **Recovery code**: 32 random bytes plus a 3-byte checksum (the first bytes
  of SHA-256 of the code), 56 Crockford characters in 8 groups of 7.
  Crockford instead of the plan's BIP39 words: one alphabet for both kit
  secrets, no word list to ship, and the checksum catches a typo before any
  network call. `recovery_wrap_key = HKDF(code, info="ottero recovery v1")`;
  `wrapped_by_recovery` uses AAD `"ottero account key recovery v1"`.
- **Dependencies** (all RustCrypto or rust-lang maintained, MIT/Apache-2.0,
  already in `Cargo.lock` through other crates, so nothing new is downloaded):
  - `hkdf` 0.12 with `sha2` 0.10: RFC 5869 HKDF-SHA256. Checked against the
    RFC's test case 1.
  - `unicode-normalization` 0.1: NFKC for passwords, so the same password
    typed on macOS, Windows or a phone keyboard derives the same key.
  - `base64` 0.22: base64url without padding for salts and wrapped keys.

## Consequences

- Changing any step locks every account out; the vectors in the tests must
  never change. They were cross-checked with an independent implementation
  (Node `crypto.hkdfSync` + `@noble/hashes` Argon2id).
- Argon2id at 256 MiB takes about a second; it runs on the background
  executor with the button's loading state visible.
- A feature `insecure-test-kdf` exposes cheap Argon2 parameters to other
  crates' tests; it is enabled only from `[dev-dependencies]`.
