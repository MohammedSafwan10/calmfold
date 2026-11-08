# 09 Packaging and releases

1. [ ] App icon (1024 px master → icns/ico)
2. [ ] macOS `.app` + `.dmg` (cargo-packager), Developer ID signing and notarization with the owner's identity (deliberate step)
3. [ ] Windows `.msi`/NSIS; Windows build host needs VS Build Tools + MSVC + CMake
4. [ ] GitHub Actions: `scripts/check.sh` on macOS + Windows; release workflow on tags
5. [ ] Auto-update (gpui-kit `auto-update` docs) once releases exist
6. [ ] Crash-safe logging to the data folder (no user content)
