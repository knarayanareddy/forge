# forge

AetherForge MVP — **Phase 3 Agent Loop** complete on Darwin (Phases 1–2 retained).

## Harness score (Darwin, canonical)

```text
cargo run -p golden-harness --bin golden-harness
→ 11/11 harness (11 hard / 0 soft)
```

Tasks: FS-01, FS-02, GIT-01, CODE-01, MCP-01, MEM-01, SKILL-01, SAFE-01, ROUT-01, RES-01, **LOOP-01**

Linux CI expects FS-02, MEM-01, and ROUT-01 to fail closed when `sandbox-exec` or Ollama are unavailable.

## Phase status

| Phase | Scope | Status |
|-------|-------|--------|
| **1 — Foundation** | Daemon, RES recovery, GIT grants, streaming ROUT | **Done** |
| **2 — MCP** | stdio JSON-RPC client, grant-gated invoke, real filesystem MCP in harness | **Done** |
| **3 — Loop** | ReAct LoopEngine, ToolRegistry, daemon loop streaming | **Done** |
| 4 — SwiftUI | macOS app + TCP client | Planned |
| 5 — Polish | DMG, notarization, CI matrix | Planned |

See [docs/ROADMAP_PHASES_1-5.md](docs/ROADMAP_PHASES_1-5.md) for acceptance criteria and audit gates.

## Layout

- Rust workspace: `Cargo.toml`, `crates/`, `tests/golden_harness/`
- **Daemon:** `crates/aether-daemon/` — TCP JSON-lines on `127.0.0.1:7433`
- Procedural skills: `skills/` (agentskills.io-style `SKILL.md`)
- Sandbox profile: `profiles/sandbox_tool.sb` (allow-default, Darwin-verified)
- MCP allowlist: `mcp_allowlist.json` (SHA-256 pins for node + server script; harness auto-discovers via `npm root -g`)
- macOS shell (stub): `macos/AetherForgeApp/` + `Package.swift`
- Specs: `docs/AetherForge_Canonical_Spec_v1.2.4.md`, `docs/ROADMAP_PHASES_1-5.md`, `docs/DAEMON_IPC.md`

## Build & evaluate

```bash
cargo build --workspace
cargo run -p golden-harness --bin golden-harness
```

**Requirements (Darwin):** `sandbox-exec` (macOS), Ollama with `all-minilm` (MEM-01) and a chat model such as `qwen2.5:3b` (ROUT-01), Node.js with `@modelcontextprotocol/server-filesystem` (MCP-01).

## MCP client (Phase 2)

`crates/aether-mcp/src/runtime.rs` provides a minimal stdio JSON-RPC MCP client:

- Load allowlist → SHA-256 verify node + entry script → spawn
- `initialize` → `tools/list` (hashes tool descriptions for audit)
- `tools/call` with JSON args
- `invoke_with_grant` enforces `mcp_call` capability before spawn

Install the filesystem MCP server if missing:

```bash
npm install -g @modelcontextprotocol/server-filesystem
```

Optional env overrides: `AETHER_MCP_NODE`, `AETHER_MCP_FILESYSTEM_SCRIPT`.

## Agent loop (Phase 3)

`crates/aether-core/src/loop_engine.rs` implements a ReAct turn loop:

- **Plan → tool → observe → verify** with `max_iterations` guard
- **ToolRegistry:** `fs_write`, `fs_read`, `python_lint`, `git_init`, `mcp_call`, `skill_execute`, `verify_contains`
- **Verifier:** CODE-01 style `PythonLintVerifier`
- **StopHook:** `GoalStopHook` for goal substring match

Daemon `run_task` accepts a JSON loop plan in `prompt`:

```json
{
  "method": "run_task",
  "params": {
    "session_id": "loop-demo",
    "workspace_path": "/tmp/aether-loop",
    "max_iterations": 8,
    "prompt": "{\"loop\":[{\"action\":\"fs_write\",\"path\":\"hello.txt\",\"content\":\"LOOP\"},{\"action\":\"verify_contains\",\"path\":\"hello.txt\",\"text\":\"LOOP\"},{\"action\":\"done\"}]}"
  }
}
```

Streamed events: `plan`, `tool`, `observe`, `verify`, `done`, `error`. Plain-text prompts still stream LLM tokens (backward compatible).

```bash
# Terminal 1
cargo run -p aether-daemon

# Terminal 2 — structured loop (requires workspace_path)
printf '%s\n' '{"method":"run_task","params":{"session_id":"loop-demo","workspace_path":"/tmp/aether-loop","prompt":"{\"loop\":[{\"action\":\"fs_write\",\"path\":\"marker.txt\",\"content\":\"ok\"},{\"action\":\"done\"}]}"}}' | nc 127.0.0.1 7433
```

## Daemon streaming (Phase 1)

```bash
# Terminal 1
cargo run -p aether-daemon

# Terminal 2 — ping
printf '%s\n' '{"method":"ping","params":{}}' | nc 127.0.0.1 7433

# Terminal 2 — streamed completion
cargo run -p aether-daemon --example stream_client
```

Full protocol: [docs/DAEMON_IPC.md](docs/DAEMON_IPC.md)

## Architecture target vs product readiness

- **Spec engineering target:** 8.5+ (see v1.2.3 / v1.2.4 docs)
- **Shipped harness baseline:** 11/11 on Darwin (11 hard) — secure FS, sandbox, crash recovery (SIGTERM child), git grant enforcement, SSE streaming router, memory (sqlite-vec KNN), MCP stdio JSON-RPC, procedural skills, permissions, Python lint, ReAct loop, daemon IPC
