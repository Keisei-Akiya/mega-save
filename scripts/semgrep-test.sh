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
# Must catch rclone spawn outside interpreter
if not any("no-rclone-command-outside-interpreter" in i for i in ids):
    print("missing expected rule hit: no-rclone-command-outside-interpreter", file=sys.stderr)
    sys.exit(1)
if len(results) < 1:
    print("expected at least 1 finding on fixtures", file=sys.stderr)
    sys.exit(1)
print("fixtures correctly flagged")
PY
