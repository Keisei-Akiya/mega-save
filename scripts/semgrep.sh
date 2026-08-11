#!/usr/bin/env bash
# Architecture gate for mega-save (Semgrep).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export PATH="${HOME}/.local/bin:${PATH}"

if ! command -v semgrep >/dev/null 2>&1; then
  echo "semgrep not found. Install: uv tool install semgrep" >&2
  exit 127
fi

TARGETS=("${ROOT}/storage" "${ROOT}/x")
JSON=$(mktemp)
trap 'rm -f "$JSON"' EXIT

echo "+ semgrep → ${TARGETS[*]}"
semgrep \
  --config "${ROOT}/semgrep/rules/" \
  --metrics=off \
  --disable-version-check \
  --json \
  --output "$JSON" \
  "${TARGETS[@]}"

# Count blocking findings via python (no jq required)
python3 - "$JSON" <<'PY'
import json, sys
path = sys.argv[1]
data = json.load(open(path))
results = data.get("results") or []
errors = data.get("errors") or []
print(f"findings={len(results)} errors={len(errors)}")
for r in results:
    print(f"  - {r.get('check_id')}: {r.get('path')}:{r.get('start',{}).get('line')}")
if errors:
    for e in errors:
        print(f"  error: {e}", file=sys.stderr)
    sys.exit(2)
if results:
    sys.exit(1)
print("semgrep architecture: OK")
PY
