#!/usr/bin/env bash
# Measures typing and scrolling in the real window with GPUI's profiler.
#
#   scripts/perf.sh [perf|release|debug] [scenario-filter]   # default: perf
#   PERF_SAMPLE=1 scripts/perf.sh perf 50-kb-typing         # + call-stack samples
#
# For each note size (1.5 KB … 2 MB) it launches Ottero on that note with
# the caret at the end, types 200 characters at 40 ms (fast typing, autosave
# runs during pauses), then scrolls, and writes one JSON report per run to
# target/perf-reports/<profile>-<scenario>.json. Needs Accessibility permission and an
# awake display; nothing touches your real data (it uses /tmp/ks-perf).
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
profile="${1:-perf}"   # perf = release speed + symbols (Cargo.toml [profile.perf])
flag="--profile $profile"
[ "$profile" = debug ] && flag="" && profile_dir=debug || profile_dir="$profile"
bin="target/$profile_dir/ottero"
only="${2:-}"          # optional: run only scenarios whose name contains this
data=/tmp/ks-perf
out=target/perf-reports
mkdir -p "$out"

cargo build -q -p ottero-desktop --features perf $flag
rm -rf "$data"
pkill -f "$bin" 2>/dev/null || true       # nothing may hold the database
seeded=$(cargo run -q -p ottero-storage --example perf_seed -- "$data/ottero.sqlite3" | wc -l)
[ "$seeded" -eq 4 ] || { echo "perf seed failed ($seeded notes)" >&2; exit 1; }

run() { # name, note title, then a driver command
  local name="$1" title="$2"; shift 2
  if [ -n "$only" ] && [[ "$name" != *"$only"* ]]; then return; fi
  local report="$out/$profile-$name.json"
  rm -f "$report"
  OTTERO_DATA_DIR="$data" OTTERO_DEV_ROUTE=notes OTTERO_DEV_NOTE="$title" \
    OTTERO_PERF_OUT="$report" "$bin" >/dev/null 2>&1 &
  local pid=$!
  # Window up and the note laid out; big notes take longer to open.
  case "$title" in *"500 KB"*) sleep 7 ;; *"2 MB"*) sleep 30 ;; *) sleep 4 ;; esac
  # The app activates itself on launch with the note's editor focused. Don't
  # "set frontmost" from outside: that activates the app without a key window,
  # and typed keys then go nowhere.
  # A full-screen app elsewhere can leave the window on another Space: step
  # through Spaces (⌃← / ⌃→) until it is on screen.
  local tries=0
  until swift scripts/window-visible.swift; do
    tries=$((tries + 1))
    [ $tries -gt 6 ] && { echo "$name: window never became visible, skipped"; kill "$pid"; return; }
    local key=$([ $tries -le 3 ] && echo 123 || echo 124)
    osascript -e "tell application \"System Events\" to key code $key using control down" >/dev/null 2>&1 || true
    sleep 1.2
  done
  sleep 1
  if [ -n "${PERF_SAMPLE:-}" ]; then           # call stacks while the driver runs
    sample "$pid" 8 -file "$out/$profile-$name.sample.txt" >/dev/null 2>&1 &
  fi
  [ -n "${PERF_DEBUG:-}" ] && echo "front: $(osascript -e 'tell application "System Events" to get unix id of first process whose frontmost is true') pid: $pid"
  "$@"
  sleep 1.5                                 # autosave + last report
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  printf '%-28s ' "$name"
  python3 - "$report" <<'EOF'
import json, sys
r = json.load(open(sys.argv[1]))
l, f = r["input_latency_ms"], r["frames"]
if l["count"] < 20:
    print(f'INVALID: only {l["count"]} input frames reached the window'); sys.exit(0)
print(f'input p50 {l["p50"]:6.2f}  p95 {l["p95"]:6.2f}  p99 {l["p99"]:6.2f}  max {l["max"]:6.2f} ms (n={l["count"]}, >16.7ms {l["over_16_7ms"]})'
      f' | draw p95 {f["draw_ms"]["p95"]:6.2f} max {f["draw_ms"]["max"]:6.2f} ms'
      f' | late {f["late"]}/{f["presented"]} frames, missed refreshes {f["missed_refreshes"]}')
EOF
}

for note in "Perf 1 KB" "Perf 50 KB" "Perf 500 KB" "Perf 2 MB"; do
  slug=$(echo "$note" | tr ' ' '-' | tr '[:upper:]' '[:lower:]')
  run "$slug-typing" "$note" swift scripts/type.swift text 200 40
  run "$slug-scroll" "$note" swift scripts/type.swift scroll 120 16 -60
done
echo "DONE. Reports: $out/$profile-*.json"
