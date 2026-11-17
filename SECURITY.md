# Security

Ottero is early software. The latest published release is the supported target
for fixes; there is no long-term support branch or formal audit claim.

## Report privately

Use [GitHub's private vulnerability reporting](https://github.com/MohammedSafwan10/ottero/security/advisories/new).
It is enabled for this repository. Do not disclose a vulnerability in a public
issue while it is being investigated.

Include the affected version and platform, impact, and a minimal reproduction
with fictional data. Do not attach a real vault, master password, recovery key,
private signing key or personal database. The maintainer will review the report;
this small project does not promise a fixed response deadline or bug bounty.

Ordinary bugs and feature requests belong in
[Issues](https://github.com/MohammedSafwan10/ottero/issues).

## What is protected

Vault contents use Argon2id and XChaCha20-Poly1305. Notes, tasks and calendar data
are local SQLite data and are **not encrypted by the app**. Protect the device
with FileVault or BitLocker as appropriate. Local-first is not a replacement for
a device backup.

Update signatures are separate from operating-system publisher certificates.
Current releases do not yet have Apple Developer ID/notarization or Windows
Authenticode publisher signing. Read [the security design](docs/security.md) for
the threat model, recovery limits and update trust boundaries.
