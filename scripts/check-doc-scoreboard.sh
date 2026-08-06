#!/usr/bin/env bash
# Prevent harness/README/CI threshold drift (Phase 8.0d).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TOTAL="$(
  perl -ne 'print "$1\n" if /const TASKS: \[TaskSpec; ([0-9]+)\]/' \
    tests/golden_harness/src/main.rs
)"
if [[ -z "$TOTAL" ]]; then
  echo "Could not parse TASKS total" >&2
  exit 1
fi

require_literal() {
  local literal="$1"
  local file="$2"
  if ! grep -Fq -- "$literal" "$file"; then
    echo "Documentation consistency failure: '$literal' missing from $file" >&2
    exit 1
  fi
}

require_literal "Tasks (${TOTAL}):" README.md
require_literal "Darwin canonical ${TOTAL}/${TOTAL}" .github/workflows/ci.yml
require_literal "requires ${TOTAL}/${TOTAL}" .github/workflows/ci.yml
require_literal "Harness matrix (${TOTAL} tasks)" docs/LINUX_CI.md
require_literal 'AETHER_ROUT_TTFT_MS: "700"' .github/workflows/ci.yml
require_literal "local product target is **≤200ms**" README.md
require_literal "**700ms CI stability gate**" README.md

echo "Scoreboard/docs gate: PASS (${TOTAL} tasks; ROUT local 200ms / CI 700ms)"
