#!/usr/bin/env bash
# Compute SHA-256 pins for mcp_allowlist.json from discovered Node + filesystem MCP server.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"

NODE="${AETHER_MCP_NODE:-$(command -v node)}"
if [[ -z "${NODE}" ]]; then
  echo "ERROR: node not found" >&2
  exit 1
fi

if [[ -n "${AETHER_MCP_FILESYSTEM_SCRIPT:-}" ]]; then
  SCRIPT="${AETHER_MCP_FILESYSTEM_SCRIPT}"
else
  NPM_ROOT="$(npm root -g 2>/dev/null || true)"
  SCRIPT="${NPM_ROOT}/@modelcontextprotocol/server-filesystem/dist/index.js"
fi

if [[ ! -f "${SCRIPT}" ]]; then
  echo "ERROR: MCP filesystem script not found at ${SCRIPT}" >&2
  exit 1
fi

NODE_SHA="$(shasum -a 256 "${NODE}" | awk '{print $1}')"
SCRIPT_SHA="$(shasum -a 256 "${SCRIPT}" | awk '{print $1}')"

TOOLS_HASH="$(
  NODE="${NODE}" SCRIPT="${SCRIPT}" node <<'NODE'
const { spawn } = require("child_process");
const crypto = require("crypto");
const fs = require("fs");

const node = process.env.NODE;
const script = process.env.SCRIPT;
const child = spawn(node, [script, "/tmp"], { stdio: ["pipe", "pipe", "ignore"] });

let buf = "";
child.stdout.on("data", (d) => { buf += d; });

function send(obj) {
  child.stdin.write(JSON.stringify(obj) + "\n");
}

send({
  jsonrpc: "2.0",
  id: 1,
  method: "initialize",
  params: {
    protocolVersion: "2024-11-05",
    capabilities: {},
    clientInfo: { name: "compute-mcp-pins", version: "1" },
  },
});
send({ jsonrpc: "2.0", method: "notifications/initialized", params: {} });
send({ jsonrpc: "2.0", id: 2, method: "tools/list", params: {} });

setTimeout(() => {
  for (const line of buf.split("\n").filter(Boolean)) {
    const msg = JSON.parse(line);
    if (msg.id === 2) {
      const tools = msg.result.tools.sort((a, b) => a.name.localeCompare(b.name));
      const agg = crypto.createHash("sha256");
      for (const t of tools) {
        const dh = crypto.createHash("sha256").update(t.description || "").digest("hex");
        agg.update(t.name);
        agg.update(dh);
      }
      console.log(agg.digest("hex"));
      child.kill();
      process.exit(0);
    }
  }
  console.error("tools/list did not return");
  process.exit(1);
}, 8000);
NODE
)"

cat <<JSON
{
  "command": "${NODE}",
  "sha256_pin": "${NODE_SHA}",
  "entry_script": "${SCRIPT}",
  "entry_sha256_pin": "${SCRIPT_SHA}",
  "tools_hash_pin": "${TOOLS_HASH}"
}
JSON
