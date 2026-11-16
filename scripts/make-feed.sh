#!/usr/bin/env bash
# Writes the signed update feed for a release: stable.json (version, notes,
# and for each platform its download URL, SHA-256 and size) and
# stable.json.minisig (minisign signature the app checks with its built-in
# public key). The app trusts a download only if the feed's signature is
# valid and the file's SHA-256 matches.
# Usage: scripts/make-feed.sh <version> <dir with Calmfold-*.zip> <minisign secret key file>
set -euo pipefail
version="$1"; dir="$2"; key="$3"
base="https://github.com/MohammedSafwan10/calmfold-releases/releases/download/v$version"
notes=$(python3 - "$version" <<'PY'
import json, sys, pathlib
log = json.loads(pathlib.Path("website/src/data/changelog.json").read_text())
entry = next((e for e in log if e["version"] == sys.argv[1]), {"date": "", "items": []})
print(json.dumps(entry))
PY
)
python3 - "$version" "$dir" "$base" "$notes" > "$dir/stable.json" <<'PY'
import hashlib, json, os, sys
version, d, base, notes = sys.argv[1], sys.argv[2], sys.argv[3], json.loads(sys.argv[4])
platforms = {}
for name in sorted(os.listdir(d)):
    for plat in ("windows-x64", "macos-arm64"):
        if name == f"Calmfold-{version}-{plat}.zip":
            data = open(os.path.join(d, name), "rb").read()
            platforms[plat] = {"url": f"{base}/{name}", "sha256": hashlib.sha256(data).hexdigest(), "size": len(data)}
if not platforms:
    sys.exit("no Calmfold zips found")
print(json.dumps({"version": version, "date": notes["date"], "notes": notes["items"], "platforms": platforms}, indent=2))
PY
minisign -S -s "$key" -m "$dir/stable.json" -x "$dir/stable.json.minisig" -t "calmfold $version" >/dev/null
echo "$dir/stable.json"
