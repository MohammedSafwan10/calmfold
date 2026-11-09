# 0006 Vault recovery: recovery key and "start a new vault"

Status: accepted (2026-11-09)

## Context

A forgotten master password can't be reset: Keepstone never has it. Users
still need a way forward that never silently loses data.

## Decision

- **Recovery key.** Setup makes 160 random bits shown as 8 groups of base32
  (`core::vault::recovery`). The app derives a second KEK from it (Argon2id at
  `KdfParams::MINIMUM`: the key is random, stretching adds nothing) and wraps
  the same vault key again (`AAD = "keepstone-vault-recovery-v1"`, migration
  0006). Setup shows it once with Copy and "Save Emergency Kit…"; Continue
  needs "I saved it". Settings → Passwords can make a new key (old one stops
  working). The key is never stored.
- **Recover.** Locked screen → "Forgot master password?" → Use recovery key →
  new master password. The vault key is re-wrapped; items are untouched.
- **Start a new vault.** For no key: the encrypted vault is written to
  `vault-archives/vault-<date>.ksvault` (header + sealed blobs, hex), read back
  and checked, and only then removed. Settings → Passwords → Archived vaults
  opens one with its old password and adds its items (new IDs) to the
  current vault. Lock state machine gains `Locked ──Reset──▶ Uninitialized`.

## Consequences

Anyone with the recovery key can open the vault; the kit says so. Old vaults
created before this have no key until the user makes one in Settings.
