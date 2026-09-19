#!/usr/bin/env bash
# Verify architecture rules fire on intentional violations (fixtures/).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="${HOME}/.local/bin:${PATH}"
cd "$ROOT"

if ! command -v semgrep >/dev/null 2>&1; then
  echo "semgrep not found" >&2
  exit 127
fi

JSON=$(mktemp)
trap 'rm -f "$JSON"' EXIT

semgrep \
  --config "${ROOT}/semgrep/rules/" \
  --metrics=off \
  --disable-version-check \
  --json \
  --output "$JSON" \
  "${ROOT}/semgrep/fixtures"

python3 - "$JSON" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
results = data.get("results") or []
ids = {r.get("check_id", "") for r in results}
print(f"fixture_findings={len(results)}")
for r in results:
    print(f"  - {r.get('check_id')}")
# Must catch rclone spawn outside interpreter and process spawns in a site module.
expected = [
    "no-rclone-command-outside-interpreter",
    "site-no-tokio-process",
    "site-no-std-process-command",
]
for suffix in expected:
    if not any(suffix in rule_id for rule_id in ids):
        print(f"missing expected rule hit: {suffix}", file=sys.stderr)
        sys.exit(1)
if len(results) < len(expected):
    print(f"expected at least {len(expected)} findings on fixtures", file=sys.stderr)
    sys.exit(1)
print("fixtures correctly flagged")
PY
