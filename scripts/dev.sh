#!/usr/bin/env bash
# Runs the dev app and rebuilds + relaunches it whenever source changes.
#
#   scripts/dev.sh              # your dev data ("Keepstone Dev" folder)
#   scripts/dev.sh --sample     # throwaway folder filled with sample notes/tasks
#
# Rust has no true hot reload; a change costs a few seconds of incremental
# build, then the window reopens. Notes autosave and layout persists, so you
# land back where you were (use KEEPSTONE_DEV_ROUTE=notes|tasks|settings to pick a screen).
# Needs watchexec:  cargo install watchexec-cli --locked
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

if [ "${1:-}" = "--sample" ]; then
  export KEEPSTONE_DATA_DIR="${TMPDIR:-/tmp}/keepstone-sample"
  if [ ! -f "$KEEPSTONE_DATA_DIR/keepstone.sqlite3" ]; then
    cargo run -q -p keepstone-storage --example seed -- "$KEEPSTONE_DATA_DIR/keepstone.sqlite3"
  fi
fi

command -v watchexec >/dev/null || { echo "Install watchexec: cargo install watchexec-cli --locked" >&2; exit 1; }

exec watchexec \
  --restart \
  --watch apps --watch crates \
  --exts rs,toml,json,sql \
  --debounce 300ms \
  -- cargo run -q -p keepstone-desktop
