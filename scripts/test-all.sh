#!/usr/bin/env bash
# Run every automated check: formatting-independent lint, unit tests, the hub
# end-to-end test and the interface end-to-end tests (Edge or Chrome).
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

echo "== interface build (includes type check)"
pnpm --filter ui build

echo "== clippy"
cargo clippy -p zaklon-core -p zaklon-hub --all-targets -- -D warnings

echo "== unit + hub end-to-end tests"
cargo test -p zaklon-core -p zaklon-hub

echo "== interface end-to-end tests"
cargo build -p zaklon-hub
pnpm --filter ui e2e

echo "All checks passed."
