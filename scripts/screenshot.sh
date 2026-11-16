#!/usr/bin/env bash
# Captures only the Calmfold window (never the whole screen) to the given path.
# Usage: scripts/screenshot.sh /tmp/calmfold.png
# Optional CALMFOLD_WINDOW_PID targets a disposable review app when two run.
set -euo pipefail
out="${1:-/tmp/calmfold.png}"
here="$(cd "$(dirname "$0")" && pwd)"
id="$(swift "$here/window-id.swift" calmfold)" || { echo "Calmfold window not found" >&2; exit 1; }
screencapture -x -o -l "$id" "$out"
echo "$out"
