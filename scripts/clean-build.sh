#!/usr/bin/env bash
# Keeps target/ from growing: deletes build files not used for 7 days
# (stale incremental sessions, old dependency versions, old test binaries).
# Usage: scripts/clean-build.sh [days]   (default 7)
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
days="${1:-7}"
before=$(du -sh target 2>/dev/null | cut -f1 || echo 0)
if command -v cargo-sweep >/dev/null; then
  cargo sweep --time "$days" >/dev/null
else
  echo "cargo-sweep not installed (cargo install cargo-sweep); removing old incremental caches only"
  find target -path '*/incremental/*' -maxdepth 4 -type d -mtime +"$days" -prune -exec rm -rf {} + 2>/dev/null || true
fi
echo "target: $before -> $(du -sh target 2>/dev/null | cut -f1)"
