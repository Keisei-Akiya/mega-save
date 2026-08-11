#!/usr/bin/env bash
# Apply rustfmt to the workspace.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# shellcheck source=/dev/null
source "${ROOT}/scripts/env-build.sh" 2>/dev/null || true
export PATH="${HOME}/.cargo/bin:${HOME}/.local/bin:${PATH}"

exec cargo fmt --all "$@"
