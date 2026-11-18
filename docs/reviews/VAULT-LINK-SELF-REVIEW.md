# Self-review: Passwords under the Ottero account key (2026-11-18)

Scope: the diff that adds ADR 0014 (commits "Wrap the vault key with the
Ottero account key …" through "Tighten Passwords account copy …").
Reviewer: the author, so this is not an independent review. Every finding
below was checked against the code, not assumed.

## Key separation and AADs

- The account key is never a cipher key. `vault_link_key = HKDF-SHA256(
  account_key, no salt, "ottero vault v1")`. Other HKDF uses of related
  secrets have different inputs or `info` (`ottero auth v1`, `ottero wrap v1`,
  `ottero recovery v1`, `ottero secret key v1`). OK.
- AAD `"ottero vault key account v1"` differs from every other wrapping of a
  key (`keepstone-vault-key-v1`, `keepstone-vault-recovery-v1`,
  `ottero account key v1`, `ottero account key recovery v1`). A test seals
  the vault key under the link key with each of those AADs and under the raw
  account key; none opens. OK.
- The envelope is the existing v1 format, fresh 24-byte nonce from the OS
  RNG every time (a test checks two wraps differ). Unwrap refuses anything but
  a 32-byte plaintext (`Format`). Errors don't say wrong key vs tampering. OK.
- **Low:** the AAD does not bind the vault's identity or the stored email.
  Swapping `account_wrapped` between two vaults of the same account opens the
  other vault's key, so every item fails its own AEAD (item AAD = id) and is
  counted unreadable; nothing decrypts wrongly and nothing is deleted. Editing
  `account_email` only changes which screen is shown (denial of service by
  someone who can already write the data folder). Accepted.

## Atomicity and "never strand a vault"

- Linking writes once: `Store::link_vault_to_account` runs the optional new
  recovery wrapping and the account wrapping + master retirement in one
  transaction, with `WHERE wrapped_key = <the wrapping just opened>` and
  `recovery_* IS NOT NULL`. Anything else rolls back. The new account
  wrapping is unwrapped and compared in memory before the write.
- Migration 0012 CHECKs make a vault without a way in unrepresentable:
  master columns all-or-nothing; email and wrapping together; master **or**
  (account + recovery). Tests try each forbidden shape.
- Crash simulation: an abort trigger on the second statement (file database,
  reopened afterwards) leaves the original header, including no recovery
  write. A link racing a password change writes nothing. A link that finishes
  after a lock, or after an account switch, is dropped (`Stale`).
- The migration copies `vault_meta` row-for-row (test: a v11 header and item
  are byte-identical after upgrading). `vault_items` is untouched.

## Downgrade paths

- Signing out, a revoked session, or signing in as someone else never
  re-enables a master password; only the recovery key can (`Recover`), and it
  removes the account wrapping in the same write. Pinned by
  `a_linked_vault_never_falls_back_to_the_master_password_by_itself` and the
  opener table test.
- Recovery with an unlocked account signed in moves the vault to that
  account. This is deliberate (whoever holds the recovery key owns the
  vault) and is stated in the recovery dialog.
- Older app builds can't open a schema-12 database (rusqlite_migration
  "too far ahead"; `run()` exits with a message) and refuse archive version 2
  files, so they can't misread a linked vault or "Start a new vault" on it.
  **Consequence (medium, operational):** installing an older version after
  this one stops the app from starting until it is upgraded again. The
  pre-update SQLite snapshot (plan 13) is the way back; there is no automatic
  rollback.
- Encrypted backups of a linked vault (version 2) carry the recovery wrapping
  and open with the recovery key. Version-1 backups never did. That widens
  who can open a backup to "account or recovery key", the same set that can
  open the live vault. Documented in security.md.

## What is in memory and on disk

- Memory: the account key only in `AccountSession` (unchanged); the link key
  is derived per use and zeroed on drop; the vault key in `VaultSession`
  (unchanged); intermediate key bytes go through `Zeroizing`. Locking drops
  both keys and every decrypted item.
- Disk: `vault_meta` gains the email and the account wrapping (ciphertext);
  the master-password salt/params/wrapping are removed on link. The account
  key, the link key and the vault key are never written.
- **Low:** key comparisons in `account_wrap`/`write_link` use `!=`, not a
  constant-time compare. Both sides are local secrets with no attacker-timed
  oracle. Accepted.

## Logs and messages

- One new log line: `couldn't open the vault with the account: {err}`. The
  error is a state or storage error (no key, email or item data). Errors shown
  in the UI are fixed sentences or anyhow chains of storage/state errors; none
  contain secrets.
- **Low (dev only):** `OTTERO_DEV_ACCOUNT=<password>` (debug builds) unlocks
  the account at startup for screenshots, like the existing
  `OTTERO_DEV_VAULT`. The password sits in the process environment of that
  debug run. Never compiled into release builds.

## Behaviour gaps found (not fixed here)

- **Low:** auto-lock ticks only while the vault is unlocked. An account key
  that is open while the vault stays locked (no vault, another account's
  vault, a key that doesn't open it) is not locked by idle time. Lock in
  background, ⌃⌘L and sign-out still lock it.
- **Low (UX):** "Lock when Ottero is in the background" now also locks the
  account key on every app switch, so Settings → Account asks for the
  password again. Intended ("one lock"), but it may surprise someone without
  a vault.
- **Info:** account-key rotation (plan 06 step 8) must re-wrap the vault key
  in the same change, or linked vaults stop opening. Noted in ADR 0014 and the
  plan.

## Fixed during the review

- Enter in the link, change-master-password, forgot-password and open-archive
  dialogs triggered the dialog's own Confirm and closed it, dropping what was
  typed (found by a UI test). They now set `on_ok(|_, _, _| false)` so the
  fields submit themselves, as the account dialogs already did. The link and
  change-password dialogs also focus their first field after opening.
