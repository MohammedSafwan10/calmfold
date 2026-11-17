#!/usr/bin/env bash
# Build and verify the shared catalog. Both native platform packages are required.
# Usage: scripts/make-feed.sh VERSION DIRECTORY MINISIGN_PRIVATE_KEY_FILE
set -euo pipefail
cd "$(dirname "$0")/.."
python3 scripts/release-metadata.py "$1" "$2"
minisign -S -s "$3" -m "$2/stable.json" -x "$2/stable.json.minisig" -t "ottero $1" >/dev/null
public="$(python3 -c 'import re; print(re.search(r"pub const RELEASE_KEY: &str = \"([^\"]+)\"", open("crates/core/src/update.rs").read())[1])')"
minisign -V -P "$public" -m "$2/stable.json" -x "$2/stable.json.minisig"
