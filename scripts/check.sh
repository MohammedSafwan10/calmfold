#!/usr/bin/env bash
# The full local quality gate. Run before every commit; CI runs the same.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

echo "==> fmt";    cargo fmt --all --check
echo "==> clippy"; cargo clippy --workspace --all-targets --locked -- -D warnings
echo "==> test";   cargo test --workspace --locked
echo "All checks passed."
