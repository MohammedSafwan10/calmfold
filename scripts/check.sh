#!/usr/bin/env bash
# The full local quality gate. Run before every commit; CI runs the same.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

# CI sets OTTERO_CHECK_PROFILE=ci (Cargo.toml) for faster cold builds.
profile="${OTTERO_CHECK_PROFILE:-dev}"

echo "==> fmt";    cargo fmt --all --check
echo "==> clippy"; cargo clippy --workspace --all-targets --locked --profile "$profile" -- -D warnings
echo "==> test";   cargo test --workspace --locked --profile "$profile"
echo "All checks passed."
