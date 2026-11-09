#!/usr/bin/env bash
# Builds target/release/Calmfold.app (release build, Calmfold icon).
# Not code-signed on purpose (AGENTS.md rule 11); macOS may ask you to
# right-click → Open the first time.
# Usage: scripts/bundle-mac.sh [--debug]
set -euo pipefail
cd "$(dirname "$0")/.."
profile=release
flag=--release
if [[ "${1:-}" == "--debug" ]]; then profile=debug; flag=; fi
cargo build $flag -p calmfold-desktop --bin calmfold
version="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
app="target/$profile/Calmfold.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$profile/calmfold" "$app/Contents/MacOS/Calmfold"
cp assets/brand/Calmfold.icns "$app/Contents/Resources/Calmfold.icns"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Calmfold</string>
  <key>CFBundleDisplayName</key><string>Calmfold</string>
  <key>CFBundleIdentifier</key><string>com.nexdark.calmfold</string>
  <key>CFBundleExecutable</key><string>Calmfold</string>
  <key>CFBundleIconFile</key><string>Calmfold</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>© Nexdark Labs</string>
</dict>
</plist>
EOF
echo "$app"
