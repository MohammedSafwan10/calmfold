#!/usr/bin/env bash
# Captures only the Ottero window (never the whole screen) to the given path.
# Usage: scripts/screenshot.sh /tmp/ottero.png
# Optional OTTERO_WINDOW_PID targets a disposable review app when two run.
set -euo pipefail
out="${1:-/tmp/ottero.png}"
here="$(cd "$(dirname "$0")" && pwd)"
id="$(swift "$here/window-id.swift" ottero)" || { echo "Ottero window not found" >&2; exit 1; }
screencapture -x -o -l "$id" "$out"
echo "$out"
