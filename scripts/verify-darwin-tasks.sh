#!/usr/bin/env bash
# Verify each golden-harness task independently (Darwin canonical gate helper).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export AETHER_MCP_NODE="${AETHER_MCP_NODE:-$(command -v node)}"
if [[ -z "${AETHER_MCP_FILESYSTEM_SCRIPT:-}" ]]; then
  npm_root="$(npm root -g 2>/dev/null || true)"
  export AETHER_MCP_FILESYSTEM_SCRIPT="${npm_root}/@modelcontextprotocol/server-filesystem/dist/index.js"
fi

TASKS=(
  ROUT-01 FS-01 FS-02 SB-01 GIT-01 CODE-01 MCP-01 MEM-01 MEM-02 GRAPH-01
  SKILL-01 SKILL-02 SAFE-01 RED-01 RES-01 LOOP-01 LOOP-02 PLAN-01 LOOP-04
  SESS-01 UNDO-01 AUTO-01 CHECK-01 GATE-01 GATE-02 HOOK-01 CKPT-01 CONS-01
  PERM-02 SUB-01 SEC-01 SKILL-03 INJECT-01 INGEST-01 BUDG-01 COST-01 GRAPH-02
  REG-01 SLEEP-01 RELY-01 FORENSIC-01 FORK-01 HEAD-01 CACHE-01 DIST-01
  MCP-02 COMPACT-01 HOOK-02 MEM-03 MCPS-01 OFFLINE-01
)

passed=0
hard=0
soft=0
failed=()
log="/tmp/golden-task-batch.log"
: >"$log"

for task in "${TASKS[@]}"; do
  printf '[%-12s] ' "$task"
  if out=$(AETHER_HARNESS_TASK="$task" ./target/debug/golden-harness 2>&1); then
    if grep -q "PASS \\[hard\\]" <<<"$out"; then
      echo "PASS [hard]"
      hard=$((hard + 1))
    elif grep -q "PASS \\[soft\\]" <<<"$out"; then
      echo "PASS [soft]"
      soft=$((soft + 1))
    elif grep -q "PASS" <<<"$out"; then
      echo "PASS"
    else
      echo "FAIL (no PASS line)"
      failed+=("$task")
      echo "$out" >>"$log"
      continue
    fi
    passed=$((passed + 1))
  else
    reason=$(grep -E "FAIL \\(|FAIL-CLOSED" <<<"$out" | tail -1 || echo "unknown")
    echo "$reason"
    failed+=("$task")
    echo "=== $task ===" >>"$log"
    echo "$out" >>"$log"
  fi
done

total=${#TASKS[@]}
echo
echo "=== Batch summary ==="
echo "Passed: $passed / $total"
echo "Hard green: $hard | Soft green: $soft"
if ((${#failed[@]})); then
  echo "Failed: ${failed[*]}"
  exit 1
fi
