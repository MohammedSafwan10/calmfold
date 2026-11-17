# 0012 Where the Ottero Cloud sign-in secrets live

Status: accepted (2026-11-17).

## Context

A signed-in device keeps two secrets: the session token and the Secret Key
(per email). Neither opens data on its own (the password is also needed),
but both should be in OS-protected storage where the OS offers it without
hurting the experience. The account key is never written anywhere.

## Decision

- **Windows release builds:** Windows Credential Manager, one generic
  credential `Ottero Cloud sign-in`, persistence Local (this computer, not
  roamed). Credential Manager encrypts it with the user's Windows login
  (DPAPI). Crates: `keyring-core` 1 and `windows-native-keyring-store` 1
  (no default features), the store crates the keyring project recommends for
  apps instead of `keyring` itself. They are Windows-only dependencies.
- **macOS release builds:** stay in `account.json` (0600) until releases are
  Developer ID signed. Keychain items are bound to the app's code signature;
  Ottero's ad-hoc signature changes with every update, so each update would
  show a keychain password prompt. With a Developer ID signature the secrets
  move to the Data Protection keychain through the same `SecretSlot` seam.
- **Development builds:** always `account.json`, never a system store
  (docs/security.md, Keychain policy).
- Older files are migrated: secrets found in `account.json` are moved into
  the store on first open and the file is rewritten without them. If the
  store can't be read, Ottero keeps using the file and deletes nothing.

## Consequences

- On Windows a copied `account.json` no longer carries the token or Secret
  Key. On macOS the file still does; FileVault and the 0600 mode protect it.
- Windows CI round-trips a throwaway credential
  (`OTTERO_TEST_WINDOWS_CREDENTIALS=1`).
