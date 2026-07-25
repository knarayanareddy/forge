#!/usr/bin/env bash
# Static MCP allowlist gate for CI (Phase 6 pre-insert).
# Validates mcp_allowlist.json schema and SHA pin format.
# Optional: runs snyk-agent-scan (formerly mcp-scan) when uvx is available —
# AetherForge uses a custom allowlist shape, so dynamic scan is informational only.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ALLOWLIST="${ROOT}/mcp_allowlist.json"

echo "== MCP allowlist static scan =="
echo "File: ${ALLOWLIST}"

if [[ ! -f "${ALLOWLIST}" ]]; then
  echo "ERROR: mcp_allowlist.json not found at repo root" >&2
  exit 1
fi

python3 - "${ALLOWLIST}" <<'PY'
import json
import re
import sys
from pathlib import Path

path = Path(sys.argv[1])
data = json.loads(path.read_text())

if "servers" not in data or not isinstance(data["servers"], list):
    raise SystemExit("ERROR: root.servers must be a non-empty array")

if not data["servers"]:
    raise SystemExit("ERROR: servers array is empty")

allowed_policies = {"prompt_always", "prompt_once", "deny"}
sha_re = re.compile(r"^(?:[0-9a-f]{64}|REPLACE_WITH_[A-Z0-9_]+|PENDING(?:_[A-Z0-9_]+)?)$")

for i, server in enumerate(data["servers"]):
    prefix = f"servers[{i}]"
    for key in ("name", "version", "command", "args", "sha256_pin", "default_policy"):
        if key not in server:
            raise SystemExit(f"ERROR: {prefix} missing required field '{key}'")

    if not isinstance(server["args"], list) or not server["args"]:
        raise SystemExit(f"ERROR: {prefix}.args must be a non-empty array")

    if server["default_policy"] not in allowed_policies:
        raise SystemExit(
            f"ERROR: {prefix}.default_policy must be one of {sorted(allowed_policies)}"
        )

    pin = server["sha256_pin"]
    if not sha_re.match(pin):
        raise SystemExit(
            f"ERROR: {prefix}.sha256_pin must be 64-char hex, REPLACE_WITH_*, or PENDING*"
        )

    entry_pin = server.get("entry_sha256_pin")
    if entry_pin is not None and not sha_re.match(entry_pin):
        raise SystemExit(f"ERROR: {prefix}.entry_sha256_pin invalid format")

    tools_pin = server.get("tools_hash_pin")
    if tools_pin is not None and not re.fullmatch(r"[0-9a-f]{64}", tools_pin):
        raise SystemExit(f"ERROR: {prefix}.tools_hash_pin must be 64-char hex when set")

    print(f"OK: server '{server['name']}' v{server['version']} policy={server['default_policy']}")

print(f"Static validation passed ({len(data['servers'])} server(s))")
PY

# Optional dynamic scan — custom allowlist format is not Cursor/Claude MCP config.
if command -v uvx >/dev/null 2>&1; then
  echo ""
  echo "== Optional snyk-agent-scan (informational) =="
  set +e
  uvx snyk-agent-scan inspect "${ALLOWLIST}" 2>&1 | head -20
  scan_rc=$?
  set -e
  if [[ ${scan_rc} -ne 0 ]]; then
    echo "Note: snyk-agent-scan exit ${scan_rc} — expected for AetherForge custom allowlist shape"
  else
    echo "Note: snyk-agent-scan completed (may report 'no mcp servers found' for custom JSON)"
  fi
else
  echo ""
  echo "Skip: uvx not installed — static validation only (install uv for optional snyk-agent-scan)"
fi

echo ""
echo "MCP allowlist gate: PASS"
