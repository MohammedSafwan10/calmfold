# 09 Packaging and releases

1. [x] App icon (master → icns/ico/favicons, `scripts/gen-icons.py`)
2. [ ] macOS `.app` + `.dmg` (cargo-packager), Developer ID signing and notarization with the owner's identity (deliberate step)
3. [ ] Windows `.msi`/NSIS; Windows build host needs VS Build Tools + MSVC + CMake
4. [x] GitHub Actions: `scripts/check.sh` on macOS + Windows; `release.yml` on `v*` tags (unsigned zips)
5. [ ] Production auto-update: [implementation and verification plan](13-production-updates.md).
   Existing custom updater is implemented but has not passed installed-app
   upgrade tests. Prototype Sparkle/Velopack before selecting the production engine.
6. [ ] Crash-safe logging to the data folder (no user content)
