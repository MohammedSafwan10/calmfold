#!/usr/bin/env bash
# Captures only the Keepstone window (never the whole screen) to the given path.
# Usage: scripts/screenshot.sh /tmp/keepstone.png
set -euo pipefail
out="${1:-/tmp/keepstone.png}"
here="$(cd "$(dirname "$0")" && pwd)"
id="$(swift "$here/window-id.swift" keepstone)" || { echo "Keepstone window not found" >&2; exit 1; }
screencapture -x -o -l "$id" "$out"
echo "$out"
