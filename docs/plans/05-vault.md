# 05 Password vault

Read `docs/security.md` and `crates/crypto/AGENTS.md` first.

## Done

- [x] `keepstone-crypto`: Argon2id KDF with bounded params, XChaCha20-Poly1305
      versioned envelope, zeroizing keys, 14 tests (tamper, wrong key/AAD...)

## Plan

1. [ ] Domain (`core::vault`): `VaultItem` {Login, Card, SecureNote, Totp},
       fields, favorites, tags. Plaintext types never reach storage.
2. [ ] Storage: `vault_meta` (salt, kdf params, wrapped vault key, version),
       `vault_items` (id, encrypted blob, updated_at, deleted_at). Migration 0002.
       Titles are encrypted too; search runs over decrypted in-memory index only.
3. [ ] Lock state machine: `Uninitialized → Locked ⇄ Unlocked`, auto-lock on
       idle (setting), sleep, and screen lock. Pinned by a table test.
4. [ ] Setup flow: master password (strength meter, no length cap), optional
       secret key shown once with "save this" step.
5. [ ] Unlock runs Argon2 via `cx.background_spawn`; show progress, never block UI.
6. [ ] UI: item list, detail, reveal/copy with clipboard auto-clear (30 s),
       password generator, TOTP codes (`totp-rs`).
7. [ ] Quick unlock (Touch ID / Windows Hello) for **release builds only**.
8. [ ] Export: `age`-encrypted file. Import: 1Password/Bitwarden CSV.
9. [ ] Tests: lock state machine, wrong password, tampered blob, header bounds,
       UI flow setup → lock → unlock → copy.
