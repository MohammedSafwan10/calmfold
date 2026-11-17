# Security

## What we protect

Notes, tasks, events, habits, and passwords/2FA secrets on the user's own devices,
and later on a self-hosted sync server the user controls.

## Threats in scope

- Someone with a copy of the data folder or a backup (lost laptop, cloud backup).
- A compromised or curious sync server (must only ever see ciphertext).
- Other local apps reading app files (partially; see limits).
- Accidental leaks: logs, crash reports, error messages, clipboard.

Out of scope: malware running as the user while the vault is unlocked, a
compromised OS, physical attacks on unlocked RAM.

## Notes, tasks, calendar, habits

- Notes and tasks are stored **unencrypted** in SQLite in the user data folder,
  like Obsidian. Users should rely on FileVault/BitLocker for at-rest protection.
- Holiday data and personal content stay local. Updates contact GitHub; opening
  external links uses the system browser. No telemetry. No `unsafe` code.
- Errors and logs never include note bodies or passwords.
- User search input is escaped before FTS5 (`codec::fts_query`); all SQL uses
  bound parameters.

## Update boundary

The shared catalog is minisign-verified with a pinned public key before parse.
Windows packages stream through a 32 KiB buffer with signed length/SHA-256
validation before Velopack installs them. macOS Sparkle separately verifies its
inline-signed XML appcast and Ed25519 archive before extraction. The appcast is
pinned to the same release as the common catalog. Persisted highest verified
version rejects older catalog replay; this is not signed metadata expiration.

Automatic checks/downloads default on. Checks start after the first window and
retry with bounded backoff. Restart refuses failed note/Developer/Passwords saves,
unapplied secret imports, open task sheets and active focus timers. A private
SQLite online-backup snapshot includes WAL data before the handoff. Notes/tasks
in that snapshot are unencrypted, like the original database; vault rows remain
encrypted. There is no automatic database rollback or new-version health monitor.

Sparkle owns Mac installation/relaunch; Velopack owns Windows installation/relaunch.
The old custom extraction/swap installer is removed. A per-data-directory OS lock
prevents two application processes; locks release on crash without stale-timeout
heuristics. A stale legacy cache is not trusted or executed.

Update keys are separate from publisher identities. The Sparkle private seed is
stored outside the repo in the owner's protected release-key directory and as a
GitHub Actions secret; only its public key ships. CI verifies both private/public
key correspondences before generating the complete candidate. No existing signing
identity or system Keychain is used. Mac bundles use ad-hoc sealing, not Developer
ID/notarization; Windows has no Authenticode certificate. Initial OS warnings
remain. Compromised release credentials can authorize malicious updates.

See [plan 13](plans/13-production-updates.md) and
[ADR 0008](decisions/0008-updater-engines.md) for verified tests and remaining
publication/recovery gates. Interrupted-install and early-crash recovery must not
be described as proven merely because a clean upgrade passed.

## Vault design

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

## Vault as built

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
  hook and `unsafe` FFI is forbidden, so use "Lock when Ottero is in the
  background" for that.
- **Randomness** never falls back: if the OS RNG fails, nothing is generated
  (the generator shows an error; a stuck source stops after 1,000 tries).
- **Clipboard:** copied secrets are cleared after 15/30/90 s, only if the
  clipboard still holds them. macOS clipboard history apps may still keep a copy.
- **Memory:** decrypted items zero on drop. Text in input fields lives in
  GPUI's input state and its undo history, which Ottero clears but can't
  zero; that's within "malware while unlocked" (out of scope).
- **Health** (weak/reused/old) is computed locally with zxcvbn; no breach
  lookups, nothing leaves the device.
- **Encrypted backup** (Settings → Passwords, File menu, ⌘K) writes the
  header and sealed items to an `.otvault` file (`.cfvault` under Calmfold, `.ksvault` under Keepstone; all still open) (the archive format, ADR 0006)
  atomically and reads it back. Nothing is decrypted to write it; it opens
  with the master password at the time of export (not the recovery key).
  Restore needs an unlocked vault, adds copies with new IDs, and skips items
  identical to live ones.

## Developer projects and environment secrets

- Developer project names, links, local folder references and setup notes are
  ordinary **unencrypted** local app data, like tasks and notes. HTTP(S) links
  reject embedded usernames/passwords. Do not put credentials in those fields.
- Environment sets reuse Passwords' vault key, encrypted items, auto-lock,
  clipboard clearing, recovery and encrypted backups. Variable names, values,
  project association and environment labels are inside the encrypted item;
  neither SQLite metadata nor FTS receives them.
- Environment items use item-data format v2; regular items stay v1. The vault
  encryption envelope/KDF are unchanged. Older clients must be upgraded to read
  v2 items. Edits and backup recovery are tested with more than 30 variables.
- Import is bounded to 1 MiB, 512 variables and 64 KiB per value; duplicate
  names and malformed input are rejected with safe line-number errors.
  Parsing never executes commands or expands references. Canonical export
  preserves literal values in Ottero's documented dialect; other dotenv
  runtimes can differ. Comments and original file layout are not preserved.
- The masked single-line editor represents line breaks as `\n`, returns as
  `\r`, tabs as `\t` and backslashes as `\\`. Pasting a value escapes these
  automatically. Copy/export decode the literal value; multiline imports are
  tested through UI editing and saving.
- File-picker results carry a draft generation. Closing/replacing a draft or
  locking invalidates late results. Lock clears rows, previews and reveals;
  the existing GPUI input-memory limitations above still apply.
- Plaintext export needs confirmation and a chosen location. A temporary file
  is written/synced and published without overwriting an existing file; Unix
  permissions are 0600. This exported file is no longer protected by the vault.
  Prefer encrypted `.otvault` backups for storage/transfer.
- Export takes an unlocked snapshot only after confirmation/path selection;
  disk writing and sync run in the background. Locking before a picker returns
  prevents the export. Once an approved write starts, it completes that snapshot;
  locking cannot recall plaintext the user has chosen to export.
- Revealing a value displays literal characters (including multiline content).
  The escaped representation is for masked editing only. Failed validation or
  an unapplied import keeps the draft open when switching Passwords items.

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
