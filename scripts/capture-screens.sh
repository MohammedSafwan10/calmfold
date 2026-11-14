#!/usr/bin/env bash
# Captures the real app's screens (light and dark) for the website and the
# product video: launches Calmfold on a demo data folder, one screen at a
# time, and shoots only its window (scripts/screenshot.sh).
# Usage: scripts/capture-screens.sh <data-dir> <out-dir>
set -euo pipefail
cd "$(dirname "$0")/.."
data="${1:?data dir}"
out="${2:?out dir}"
mkdir -p "$out"
bin=target/debug/calmfold
shots=(
  "today|today||"
  "notes|notes||"
  "tasks|tasks||"
  "calendar-month|calendar|month|"
  "calendar-week|calendar|week|"
  "habits|habits||"
  "habit-insights|habits||insights"
  "passwords|passwords||"
)
for theme in light dark; do
  printf '{"theme":"%s"}\n' "$theme" > "$data/appearance.json"
  for s in "${shots[@]}"; do
    IFS='|' read -r name route cal hab <<<"$s"
    pkill -f "debug/calmfold" 2>/dev/null || true
    sleep 1
    env CALMFOLD_DATA_DIR="$data" CALMFOLD_DEV_ROUTE="$route" \
      ${cal:+CALMFOLD_DEV_CALENDAR="$cal"} ${hab:+CALMFOLD_DEV_HABITS="$hab"} \
      CALMFOLD_DEV_VAULT="plum-orbit-candle-9-meadow" \
      "$bin" >/dev/null 2>&1 &
    sleep 9
    scripts/screenshot.sh "$PWD/$out/$name-$theme.png" >/dev/null
    echo "$out/$name-$theme.png"
  done
done
pkill -f "debug/calmfold" 2>/dev/null || true
