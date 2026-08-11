#!/usr/bin/env bash
# Full quality gate: fmt check → clippy → semgrep → test
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# shellcheck source=/dev/null
source "${ROOT}/scripts/env-build.sh" 2>/dev/null || true
export PATH="${HOME}/.cargo/bin:${HOME}/.local/bin:${PATH}"

echo "==> cargo fmt --check"
cargo fmt --all -- --check

echo "==> cargo clippy -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> semgrep architecture"
bash "${ROOT}/scripts/semgrep.sh"

echo "==> cargo test"
cargo test --workspace

echo "check: OK"
