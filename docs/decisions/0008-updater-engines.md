# 0008: Use Sparkle and Velopack with native GPUI controls

The legacy ZIP extraction/file-swap installer is removed. macOS uses pinned
Sparkle 2.10.1; Windows uses pinned Velopack 1.2.161. Both consume authenticated
release metadata and preserve the existing app actions and Settings controls.

Sparkle runs in a separate background helper bundle using its documented custom
SPUUserDriver. Bounded JSON status/command pipes connect it to GPUI. Its framework
is built from checksum-pinned source with SPARKLE_BUILD_UI_BITS=0; the build scans
Mach-O linkage and rejects WebKit. Release notes stay plain text in GPUI. The
Swift bridge contains no installer implementation. It pins the appcast to the
same version as the verified common catalog and rejects disagreement.

Velopack uses our UpdateSource implementation, never the SDK's unsigned
HttpSource. It receives only a signature-verified catalog, with mandatory size
and SHA-256 checked incrementally before the SDK accepts a downloaded package.
No staging-ID query is sent. The SDK starts its native updater waiting for the
host to exit; GPUI saves, snapshots SQLite and quits normally.

Local installed Mac fixture tests prove signed upgrade, replacement, actual new
version startup and rejection of a modified archive. They exercise the shipping
nested helper layout and the core-only framework. Windows runtime proof runs on
Windows CI using two actual vpk packages and the production authenticated source;
results must pass before release publication. These are isolated fixtures, never
owner data. They do not constitute tests of every supported OS or a security audit.

Update signing is distinct from operating-system publisher signing. Ad-hoc Mac
bundle sealing uses no identity or Keychain and is not Developer ID/notarization.
Windows packages are not Authenticode-signed. First-install platform warnings
remain until separately provisioned publisher credentials exist.

[Plan 13](../plans/13-production-updates.md) tracks remaining hardening, including
power-loss/early-crash recovery, metadata expiry/key rotation and delta updates.
