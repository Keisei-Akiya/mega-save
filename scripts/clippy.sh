#!/usr/bin/env bash
# Clippy with warnings denied (CI gate).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# shellcheck source=/dev/null
source "${ROOT}/scripts/env-build.sh" 2>/dev/null || true
export PATH="${HOME}/.cargo/bin:${HOME}/.local/bin:${PATH}"

exec cargo clippy --workspace --all-targets -- -D warnings "$@"
