# 0009: Rename Calmfold to Ottero without breaking installed apps

The product is now **Ottero by Nexdark Labs** (formerly Calmfold, formerly
Keepstone). Every user-visible and code name changed: crates `ottero-*`,
binary `ottero`, `Ottero.app` with executable `Ottero` and helper
`OtteroUpdater.app`, `Ottero.exe`, `OTTERO_*` environment variables, the
"Ottero Light/Dark" themes and the `ottero://` preview link scheme.

A few identifiers keep the old name because installed Calmfold copies, their
data or their backups depend on them. Each is marked with a comment in code.

## Kept identifiers

| Identifier | Value kept | Why |
| --- | --- | --- |
| macOS `CFBundleIdentifier` | `com.nexdark.calmfold` (helper `.updater`) | Sparkle accepts an update bundle whose file name differs from the installed one only when its bundle ID matches (`Autoupdate/SUInstaller.m`, `installSourcePathInUpdateFolder`), and the installer refuses a host whose ID changed (`Autoupdate/AppInstaller.m`, "Failed to match host bundle identifiers"). Sparkle's caches and XPC service names are also keyed by it. |
| GPUI `app_id` | `com.nexdark.calmfold` | Kept equal to the bundle ID. |
| Velopack `packId` / `PackageId` | `Calmfold` | The installed identity: install folder `%LocalAppData%\Calmfold`, uninstall entry, `Calmfold-<version>-full.nupkg` and the package the SDK accepts. A new packId would be a different app that can't update the old one. |
| Vault AADs | `keepstone-vault-key-v1`, `keepstone-vault-recovery-v1` | Wrapped vault keys are bound to these bytes. |
| Archive format | `keepstone-vault-archive` | Every existing backup/archive carries it. |
| Update feed | `MohammedSafwan10/calmfold-releases` URLs, release minisign key, Sparkle EdDSA key | Repositories aren't renamed yet; GitHub redirects renamed repositories. Signing keys are unchanged. |

## Changed with a migration

- **Data folder**: `ProjectDirs("com", "Nexdark", "Ottero")` ("Ottero Dev" in
  debug). On first launch the newest old folder (Calmfold, then Keepstone) is
  moved there and `calmfold.sqlite3`/`keepstone.sqlite3` (+ `-wal`, `-shm`) is
  renamed to `ottero.sqlite3`, once, never overwriting (`paths.rs` tests).
  `CALMFOLD_DATA_DIR` is still read when `OTTERO_DATA_DIR` is unset.
- **Themes**: saved "Calmfold Light/Dark" (and "Keepstone …") load as
  "Ottero Light/Dark" (`appearance.rs` tests).
- **Vault files**: new backups and archives use `.otvault`; `.cfvault` and
  `.ksvault` (same format) still open and are listed.

## Update path, with evidence

**macOS (Sparkle 2.10.1).** The update archive contains `Ottero.app` with the
kept bundle ID. Sparkle finds it by bundle ID and installs it at the existing
path (`host.bundlePath`; `SPARKLE_NORMALIZE_INSTALLED_APPLICATION_NAME` is 0 in
`ConfigCommon.xcconfig`), then relaunches the new `CFBundleExecutable`. The old
app launches its own `CalmfoldUpdater` helper; the new bundle carries
`OtteroUpdater`, which the new app uses afterwards. `scripts/test-sparkle.py`
now installs a fixture with Calmfold names and updates it to Ottero names with
the same bundle ID; it passed locally (upgrade, relaunch, tamper rejection).
Limitation: an existing install keeps the folder name `/Applications/Calmfold.app`
(menu bar and Dock show "Ottero"). Renaming it in Finder is safe.

**Windows (Velopack 1.2.161).** `--mainExe` changes to `Ottero.exe` and
`--packTitle Ottero` is added. Source evidence from the 1.2.161 tag:
`apply.rs` restarts the *applied* package's main executable; `shortcuts.rs`
retargets shortcuts whose target no longer exists to the new main executable
and renames them when the title changes; `registry.rs` rewrites the uninstall
entry's `DisplayName`/`DisplayIcon`; the root stub (`stub.rs`) only runs
`Update.exe start`, which reads the main executable from the current manifest,
so an old `Calmfold.exe` stub still opens Ottero. The Windows CI probe
(`update-probes.yml`, `test-velopack.py`) now packs the old fixture as
`Calmfold.exe` and the new one as `Ottero.exe`; it must pass on Windows CI
before the first Ottero release. It has not run yet. Downloaded installer
files are renamed to `Ottero-win-Setup.exe` / `Ottero-win-Portable.zip` after
packing; their contents are unchanged.

## Later

Renaming the GitHub repositories and the website domain, and moving the feed
URL to them, is a separate step. Any future change of the kept identifiers needs
its own migration and installed-upgrade proof.
