#!/usr/bin/env bash
# Builds target/release/Calmfold.app (release build, Calmfold icon).
# Ad-hoc sealed without any identity or Keychain access. Not Developer ID
# signed/notarized: macOS may require right-click → Open on first installation.
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
python3 scripts/build-sparkle-core.py
mkdir -p "$app/Contents/Frameworks" "$app/Contents/Helpers/CalmfoldUpdater.app/Contents/MacOS"
ditto target/tools/sparkle/core/Sparkle.framework "$app/Contents/Frameworks/Sparkle.framework"
xcrun swiftc -swift-version 5 -warnings-as-errors -parse-as-library -O \
  -target arm64-apple-macos12.0 -F target/tools/sparkle/core -framework Sparkle -framework AppKit \
  -Xlinker -rpath -Xlinker '@executable_path/../../../../Frameworks' \
  native/macos/CalmfoldUpdater.swift -o "$app/Contents/Helpers/CalmfoldUpdater.app/Contents/MacOS/CalmfoldUpdater"
python3 - "$app" "$version" <<'PYINFO'
from pathlib import Path
import plistlib,base64,sys
app=Path(sys.argv[1]); version=sys.argv[2]
public=Path('native/macos/sparkle-public-key.txt').read_text().strip()
assert len(base64.b64decode(public,validate=True))==32
info={'CFBundleName':'Calmfold','CFBundleDisplayName':'Calmfold','CFBundleIdentifier':'com.nexdark.calmfold',
 'CFBundleExecutable':'Calmfold','CFBundleIconFile':'Calmfold','CFBundlePackageType':'APPL',
 'CFBundleShortVersionString':version,'CFBundleVersion':version,'LSMinimumSystemVersion':'12.0',
 'NSHighResolutionCapable':True,'NSHumanReadableCopyright':'© Nexdark Labs',
 'SUPublicEDKey':public,'SUFeedURL':'https://github.com/MohammedSafwan10/calmfold-releases/releases/latest/download/appcast.xml',
 'SURequireSignedFeed':True,'SUVerifyUpdateBeforeExtraction':True,'SUShowReleaseNotes':False,
 'SUEnableAutomaticChecks':False,'SUAutomaticallyUpdate':False}
(app/'Contents/Info.plist').write_bytes(plistlib.dumps(info))
helper={'CFBundleName':'Calmfold Updater','CFBundleIdentifier':'com.nexdark.calmfold.updater',
 'CFBundleExecutable':'CalmfoldUpdater','CFBundlePackageType':'APPL','CFBundleVersion':version,
 'LSMinimumSystemVersion':'12.0','LSBackgroundOnly':True}
(app/'Contents/Helpers/CalmfoldUpdater.app/Contents/Info.plist').write_bytes(plistlib.dumps(helper))
PYINFO
cp LICENSE NOTICE "$app/Contents/Resources/"
python3 scripts/release-source.py "$app/Contents/Resources"
python3 scripts/third-party-notices.py "$app/Contents/Resources/THIRD_PARTY_NOTICES.txt"
cp target/tools/sparkle/LICENSE "$app/Contents/Resources/Sparkle-LICENSE.txt"
# Ad-hoc sealing is required for bundle integrity, uses no identity or Keychain,
# and does not claim Developer ID signing or Apple notarization.
codesign --force --sign - "$app/Contents/Helpers/CalmfoldUpdater.app"
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
echo "$app"
