# forge

AetherForge MVP — **Phase 3 Agent Loop** complete on Darwin (Phases 1–2 retained).

## Harness score (Darwin, canonical)

```text
cargo run -p golden-harness --bin golden-harness
→ 11/11 harness (11 hard / 0 soft)
```

Tasks: FS-01, FS-02, GIT-01, CODE-01, MCP-01, ROUT-01, MEM-01, SKILL-01, SAFE-01, RES-01, LOOP-01

Linux CI expects FS-02, MEM-01, and ROUT-01 to fail closed when `sandbox-exec` or Ollama are unavailable.

## Phase status

| Phase | Scope | Status |
|-------|-------|--------|
| **1 — Foundation** | Daemon, RES recovery, GIT grants, streaming ROUT | **Done** |
| **2 — MCP** | stdio JSON-RPC client, grant-gated invoke, real filesystem MCP in harness | **Done** |
| **3 — Loop** | ReAct LoopEngine, ToolRegistry, daemon integration, LOOP-01 | **Done** |
| 4 — SwiftUI | macOS app + TCP client | Planned |
| 5 — Polish | DMG, notarization, CI matrix | Planned |

See [docs/ROADMAP_PHASES_1-5.md](docs/ROADMAP_PHASES_1-5.md) for acceptance criteria and audit gates.

## Layout

- Rust workspace: `Cargo.toml`, `crates/`, `tests/golden_harness/`
- **Daemon:** `crates/aether-daemon/` — TCP JSON-lines on `127.0.0.1:7433`
- **Loop engine:** `crates/aether-core/src/loop_engine.rs` — plan → tool → observe → verify
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

## Agent loop (Phase 3)

`ReActLoopEngine` runs structured plans: **plan → tool → observe → verify**, capped by `max_iterations`.

**ToolRegistry** wires grant-checked: FS read/write, git (`GitOps`), MCP (`invoke_with_grant`), skills (`SkillExecutor`), Python lint (`PythonLinter`).

Send a JSON loop plan via daemon `run_task`:

```json
{
  "method": "run_task",
  "params": {
    "session_id": "demo-loop",
    "workspace_path": "/tmp/aether-workspace",
    "max_iterations": 8,
    "prompt": "{\"loop\":[{\"tool\":\"fs_write\",\"path\":\"hello.txt\",\"content\":\"forge\"},{\"tool\":\"verify_contains\",\"path\":\"hello.txt\",\"text\":\"forge\"},{\"tool\":\"done\"}]}"
  }
}
```

Streamed events: `plan`, `tool`, `observe`, `verify`, then `done` or `error`.

Plain-text prompts (no `{"loop":...}`) still stream tokens via Ollama (Phase 1 behavior).

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

## Daemon streaming (Phase 1 + 3)

```bash
# Terminal 1
cargo run -p aether-daemon

# Terminal 2 — ping
printf '%s\n' '{"method":"ping","params":{}}' | nc 127.0.0.1 7433

# Terminal 2 — streamed completion (plain prompt)
cargo run -p aether-daemon --example stream_client

# Terminal 2 — structured loop
mkdir -p /tmp/aether-workspace
printf '%s\n' '{"method":"run_task","params":{"session_id":"loop-demo","workspace_path":"/tmp/aether-workspace","prompt":"{\"loop\":[{\"tool\":\"fs_write\",\"path\":\"out.txt\",\"content\":\"forge\"},{\"tool\":\"verify_contains\",\"path\":\"out.txt\",\"text\":\"forge\"},{\"tool\":\"done\"}]}"}}' | nc 127.0.0.1 7433
```

Full protocol: [docs/DAEMON_IPC.md](docs/DAEMON_IPC.md)

## Architecture target vs product readiness

- **Spec engineering target:** 8.5+ (see v1.2.3 / v1.2.4 docs)
- **Shipped harness baseline:** 11/11 on Darwin (11 hard) — secure FS, sandbox, crash recovery (SIGTERM child), git grant enforcement, SSE streaming router, memory (sqlite-vec KNN), MCP stdio JSON-RPC, procedural skills, ReAct loop, permissions, Python lint, daemon IPC
