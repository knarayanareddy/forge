#!/usr/bin/env bash
# consolidate_memory.sh — offline Dream job: dedupe wiki zone, surface contradictions
#
# Produces human-reviewable JSON + markdown artifacts and a consolidation_runs row
# with status=review_pending. No auto-apply.

set -euo pipefail

DB=""
SESSION=""
OUT="artifacts/consolidate"

usage() {
  cat <<'EOF'
consolidate_memory.sh — offline wiki-zone consolidate preview

  --db PATH         SQLite database path (aether.db)
  --session ID      Session id to consolidate
  --out PATH        Artifact output directory (default: artifacts/consolidate)

Writes consolidate-{run_id}.json + .md and sets consolidation_runs.status=review_pending.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --db) DB="$2"; shift 2 ;;
    --session) SESSION="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; usage; exit 1 ;;
  esac
done

if [[ -z "$DB" || -z "$SESSION" ]]; then
  echo "error: --db and --session required" >&2
  usage
  exit 1
fi

if [[ ! -f "$DB" ]]; then
  echo "error: database not found: $DB" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo run -q -p aether-db --bin consolidate-memory -- \
  --db "$DB" \
  --session "$SESSION" \
  --out "$OUT"
