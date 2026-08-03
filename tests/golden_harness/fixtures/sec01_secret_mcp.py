#!/usr/bin/env python3
"""Minimal stdio MCP server for SEC-01 — authenticates via a brokered env secret.

Protocol matches aether-mcp's newline-delimited JSON-RPC client (not Content-Length).
The `authenticate` tool reads `API_TOKEN` from the process environment (injected by the
daemon at spawn time) and deliberately echoes the raw value in its result so the harness
can prove observation/session-log redaction — the value must never survive into those sinks.
"""

from __future__ import annotations

import hashlib
import json
import os
import sys


def write(msg: dict) -> None:
    sys.stdout.write(json.dumps(msg, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def tool_list_result() -> dict:
    return {
        "tools": [
            {
                "name": "authenticate",
                "description": "Authenticate using the brokered API_TOKEN env secret",
                "inputSchema": {"type": "object", "properties": {}},
            }
        ]
    }


def authenticate_result() -> dict:
    token = os.environ.get("API_TOKEN", "")
    if not token:
        return {
            "content": [{"type": "text", "text": "authenticated=false missing_secret"}],
            "isError": True,
        }
    fingerprint = hashlib.sha256(token.encode("utf-8")).hexdigest()
    # Intentionally include the raw token so SEC-01 can prove redaction across sinks.
    text = f"authenticated=true fingerprint={fingerprint} leaked={token}"
    return {"content": [{"type": "text", "text": text}], "isError": False}


def main() -> None:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue

        method = msg.get("method")
        msg_id = msg.get("id")

        if method == "initialize":
            write(
                {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {"tools": {}},
                        "serverInfo": {"name": "sec01-secret", "version": "1.0.0"},
                    },
                }
            )
            continue

        if method == "notifications/initialized":
            continue

        if method == "tools/list":
            write({"jsonrpc": "2.0", "id": msg_id, "result": tool_list_result()})
            continue

        if method == "tools/call":
            params = msg.get("params") or {}
            name = params.get("name")
            if name == "authenticate":
                write({"jsonrpc": "2.0", "id": msg_id, "result": authenticate_result()})
            else:
                write(
                    {
                        "jsonrpc": "2.0",
                        "id": msg_id,
                        "error": {"code": -32601, "message": f"unknown tool {name}"},
                    }
                )
            continue

        if msg_id is not None:
            write(
                {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "error": {"code": -32601, "message": f"unknown method {method}"},
                }
            )


if __name__ == "__main__":
    main()
