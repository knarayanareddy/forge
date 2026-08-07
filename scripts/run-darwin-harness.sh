#!/usr/bin/env bash
# Run the full Darwin golden harness with MCP env and log to /tmp/golden-final.log
set -euo pipefail
cd "$(dirname "$0")/.."

export AETHER_MCP_NODE="${AETHER_MCP_NODE:-$(command -v node)}"
export AETHER_MCP_FILESYSTEM_SCRIPT="${AETHER_MCP_FILESYSTEM_SCRIPT:-$(npm root -g 2>/dev/null)/@modelcontextprotocol/server-filesystem/dist/index.js}"

LOG="${1:-/tmp/golden-final.log}"
echo "Logging to $LOG"
exec ./target/debug/golden-harness 2>&1 | tee "$LOG"
