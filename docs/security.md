# Security

## What we protect

Notes, tasks, and (planned) passwords/2FA secrets on the user's own devices,
and later on a self-hosted sync server the user controls.

## Threats in scope

- Someone with a copy of the data folder or a backup (lost laptop, cloud backup).
- A compromised or curious sync server (must only ever see ciphertext).
- Other local apps reading app files (partially; see limits).
- Accidental leaks: logs, crash reports, error messages, clipboard.

Out of scope: malware running as the user while the vault is unlocked, a
compromised OS, physical attacks on unlocked RAM.

## Current state (Phase 1)

- Notes and tasks are stored **unencrypted** in SQLite in the user data folder,
  like Obsidian. Users should rely on FileVault/BitLocker for at-rest protection.
- No network access. No telemetry. No `unsafe` code.
- Errors and logs never include note bodies or passwords.
- User search input is escaped before FTS5 (`codec::fts_query`); all SQL uses
  bound parameters.

## Vault design (Phase 3, crypto crate already built and tested)

- Master password → **Argon2id** v1.3, default m=64 MiB, t=3, p=4 (RFC 9106
  option 2), random 16-byte salt. Params stored per vault and bounded on read
  (19 MiB–1 GiB, t 2–64, p 1–16) so a tampered header can't weaken or hang unlock.
- Optional 128-bit **secret key** (like 1Password) mixed in, so a leaked
  server database is useless even with a weak password. Planned.
- Derived key unwraps a random **vault key**; items are sealed with
  **XChaCha20-Poly1305**, fresh 24-byte random nonce each time, record ID as
  associated data. Format is versioned (`"KS" | v1 | nonce | ct | tag`).
- Keys are `SecretKey` (zeroize on drop, redacted Debug, not Clone).
- Auto-lock on idle/sleep, clipboard auto-clear for copied passwords, no
  secrets in search index or previews.

## Vault as built (2026-11-09)

- Hierarchy and format: ADR 0005. Storage holds the header and sealed blobs
  only; titles, URLs and favorites are encrypted too.
- Unlock/setup/password change derive on a background thread; the UI never
  blocks on Argon2. A result arriving after the user locked again is ignored.
- **Locking** (manual ⌃⌘L, idle timer, background, waking from sleep) drops
  the vault key and every decrypted item, wipes unsaved drafts and the
  search field, and closes an open generator.
- **Idle** is measured on the wall clock (`SystemTime`), because `Instant`
  stops while macOS sleeps. A 10 s tick that arrives ≥ 60 s late means the
  Mac slept: the vault locks on wake.
- **Not detected:** the macOS lock screen without sleep. GPUI exposes no
  hook and `unsafe` FFI is forbidden, so use "Lock when Calmfold is in the
  background" for that.
- **Randomness** never falls back: if the OS RNG fails, nothing is generated
  (the generator shows an error; a stuck source stops after 1,000 tries).
- **Clipboard:** copied secrets are cleared after 15/30/90 s, only if the
  clipboard still holds them. macOS clipboard history apps may still keep a copy.
- **Memory:** decrypted items zero on drop. Text in input fields lives in
  GPUI's input state and its undo history, which Calmfold clears but can't
  zero; that's within "malware while unlocked" (out of scope).
- **Health** (weak/reused/old) is computed locally with zxcvbn; no breach
  lookups, nothing leaves the device.
- **Encrypted backup** (Settings → Passwords, File menu, ⌘K) writes the
  header and sealed items to a `.cfvault` file (`.ksvault` before the Calmfold rename) (the archive format, ADR 0006)
  atomically and reads it back. Nothing is decrypted to write it; it opens
  with the master password at the time of export (not the recovery key).
  Restore needs an unlocked vault, adds copies with new IDs, and skips items
  identical to live ones.

## Keychain policy

- Unsigned `cargo run` binaries change their code signature on every build,
  so macOS asks for the login keychain password each time (the "wants to
  access key … Safe Storage" prompt). To avoid that:
  - **Development builds never use the system keychain.** Quick-unlock
    secrets in debug builds go to a dev-only store or are simply not saved.
  - **Release builds** (later) use the macOS Data Protection keychain with a
    proper Developer ID signature, and Windows Credential Manager / DPAPI.
- Do **not** code-sign with whatever identity happens to be on the machine.
  Signing setup will be a deliberate release step with the owner's own identity.

## Sync (Phase 4)

End-to-end encrypted: the server stores opaque blobs and metadata needed for
ordering only. TLS via Coolify/Traefik. Account auth separate from the vault key.
