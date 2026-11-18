# 0014 Passwords under the Ottero Cloud account key

Status: accepted (2026-11-18).

## Context

Plan 06 (§2–3) and ADR 0010 promise one password: signed in to Ottero Cloud,
the account password also unlocks Passwords, and the vault's own master
password is retired on that device. The vault (ADR 0003/0005) seals every
item with a random vault key that is wrapped by the master password's
Argon2id key and, optionally, by the recovery key (ADR 0006). The account key
(ADR 0011) is 32 random bytes that exist only in memory after the account
password is entered.

These are users' saved passwords: linking must never lose or corrupt the
vault, never weaken its encryption, and never leave a vault that nothing can
open.

## Decision

- **Wrapping.** `vault_link_key = HKDF-SHA256(ikm = account_key, no salt,
  info = "ottero vault v1")`; the vault key is sealed with it in the vault's
  versioned envelope (`"KS" | 0x01 | nonce24 | ct | tag`) with AAD
  `"ottero vault key account v1"` (`ottero_crypto::vault_link`). The account
  key is never used directly as a cipher key; the `info` and the AAD keep this
  wrapping apart from the master-password (`keepstone-vault-key-v1`) and
  recovery (`keepstone-vault-recovery-v1`) wrappings and from the account
  key's own wraps. Fixed vectors were produced independently with Python
  `hmac`/`hashlib` (HKDF) and `@noble/ciphers` + `@noble/hashes` (envelope).
  No new dependency.
- **Storage.** Migration 0012 rebuilds `vault_meta` (copying every row
  unchanged) so the master-password columns can be NULL, and adds
  `account_email` + `account_wrapped`. CHECK constraints: master columns all
  set or all NULL; email and wrapping together; and a vault always keeps a
  way in: the master password, **or** the account wrapping together with a
  recovery key. No item row is touched.
- **Linking** asks for the master password once (even if the vault is already
  unlocked: an attacker at an unlocked computer must not be able to tie the
  vault to their own account, which matters once sync exists). The master key
  must open the stored wrapping; the new account wrapping is opened again in
  memory before anything is written; then one SQLite transaction stores it
  and sets the master columns to NULL, guarded by compare-and-swap on the
  master wrapping it just opened. A vault without a recovery key gets a new
  one in the same transaction and shows it once. A crash or error before the
  commit leaves the old header, so the master password keeps working.
- **Master-password wrapping: retired, not kept.** Keeping it as an
  "unlink on sign-out" fallback would leave a second, weaker opener (a
  64 MiB Argon2id password the user may have reused, or picked before
  choosing a better account password) that an attacker with a copy of the
  data folder could keep guessing, after the user was told it was retired.
  Data is not stranded because the recovery key is required, and the account
  key is recoverable through the account's own password, Secret Key and
  recovery code. Signing out never downgrades the vault to a master password;
  only the recovery key can (see below).
- **One unlock.** Whenever the account key opens (sign-in, unlock after a
  restart, any password check), a locked vault linked to that account opens
  too: HKDF + one AEAD, no Argon2. Locking Passwords (manual, idle, sleep,
  background) also locks the account key; locking the account locks
  Passwords. Signing out (or a session ended by the server) locks Passwords
  and drops the account key; the link stays, so signing in again reopens it.
- **Another account on this device.** The vault stores the email it is linked
  to. Signed in as someone else, it is not tried and nothing changes; the
  locked screen says which account opens it and offers the recovery key. With
  the same email but a key that doesn't open it (a re-created account, or a
  changed file) the AEAD fails and the screen says so; nothing is changed.
- **Recovery** (vault recovery key, ADR 0006) still works on a linked vault.
  With an unlocked account signed in, the vault then opens with that account
  (this is how a vault moves to another account: whoever holds the recovery
  key owns it). Otherwise the user chooses a new master password and the
  account wrapping is removed in the same write.
- **Backups and archives** of a linked vault are archive format version 2:
  no master password; they hold the account wrapping and the recovery
  wrapping and open with that account or the recovery key. Master-password
  vaults still write version 1 byte for byte. Older app versions refuse
  version 2 files and a database at schema 12 (rusqlite_migration "too far
  ahead"), so a downgrade can't misread or reset a linked vault.
- **State machine.** `ottero_core::vault::lock::next_opener` (documented and
  pinned by an exhaustive table test) allows `SetUp(MasterPassword|Account)`,
  `Link` (Locked or Unlocked, master-password vault only), `Recover(o)` and
  `Reset` (Locked).

## Consequences

- One password for Cloud users; the master password stops working on this
  device the moment the link commits.
- A linked vault is only as strong as the account password + Secret Key (both
  needed: Argon2id 256 MiB, ADR 0011) or the 160-bit recovery key. On macOS
  development and current release builds the Secret Key sits in
  `account.json` in the same data folder, so against a stolen folder the
  account password is the effective secret (at a higher Argon2 cost than the
  old master password).
- Losing the account password, the Emergency Kit and the vault recovery key
  together loses the vault; the app already says this about the recovery key.
- Sync (plan 06 steps 6–7) will need the vault key on other devices; that
  will be a collection key under the account key, not this local wrapping.
