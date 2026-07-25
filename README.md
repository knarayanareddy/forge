# forge

AetherForge MVP — **Phase 5 Polish** complete on Darwin (Phases 1–4 retained).

## Harness score (Darwin, canonical)

```text
cargo run -p golden-harness --bin golden-harness
→ 15/15 harness (15 hard / 0 soft) when ROUT-01 median warm TTFT ≤ 200ms, GRAPH-01 recall@3 ≥ 1.0, LOOP-02 NL plan trajectory matches gold, RED-01 blocks all 14 adversarial cases, and SKILL-02 routes 3/3 book_skill questions with citation fidelity ≥ 0.9
```

Tasks: ROUT-01, FS-01, FS-02, GIT-01, CODE-01, MCP-01, MEM-01, GRAPH-01, SKILL-01, SKILL-02, SAFE-01, RED-01, RES-01, LOOP-01, LOOP-02

ROUT-01 runs first (before FS-02 sandbox load and MCP/MEM embedder swap), pre-warms the chat model at harness start, drains five warmup streams (`keep_alive: 30m`, `num_ctx: 512`), then asserts the **median** of three Ollama server-side warm TTFT samples (`load_duration + prompt_eval_duration`) is ≤ 200ms.

Linux CI expects FS-02, MEM-01, ROUT-01, GRAPH-01, and LOOP-02 to fail closed when `sandbox-exec` or Ollama are unavailable.

## Phase status

| Phase | Scope | Status |
|-------|-------|--------|
| **1 — Foundation** | Daemon, RES recovery, GIT grants, streaming ROUT | **Done** |
| **2 — MCP** | stdio JSON-RPC client, grant-gated invoke, real filesystem MCP in harness | **Done** |
| **3 — Loop** | ReAct LoopEngine, ToolRegistry, daemon integration, LOOP-01 | **Done** |
| **4 — SwiftUI** | macOS app, TCP daemon client, workspace bookmarks | **Done** |
| **5 — Polish** | Keychain BYOK, hybrid memory RRF, DMG/notarize scripts, Linux CI | **Done** |

See [docs/ROADMAP_PHASES_1-5.md](docs/ROADMAP_PHASES_1-5.md) for acceptance criteria and audit gates.

## Layout

- Rust workspace: `Cargo.toml`, `crates/`, `tests/golden_harness/`
- **Daemon:** `crates/aether-daemon/` — TCP JSON-lines on `127.0.0.1:7433`
- **Loop engine:** `crates/aether-core/src/loop_engine.rs` — plan → tool → observe → verify
- Procedural skills: `skills/` (agentskills.io-style `SKILL.md`)
- Sandbox profile: `profiles/sandbox_tool.sb` (allow-default, Darwin-verified)
- MCP allowlist: `mcp_allowlist.json` (SHA-256 pins for node + server script; harness auto-discovers via `npm root -g`)
- macOS app: `macos/AetherForgeApp/` + `Package.swift` + `macos/AetherFFI/` (C ABI hints)
- Specs: `docs/AetherForge_Canonical_Spec_v1.2.4.md`, `docs/ROADMAP_PHASES_1-5.md`, `docs/DAEMON_IPC.md`, `docs/INSTALL.md`, `docs/LINUX_CI.md`

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

## macOS app (Phase 4)

Build the Rust FFI staticlib, then the Swift app:

```bash
./scripts/build-ffi.sh
swift build
swift run AetherForgeApp
```

**Run the stack:**

```bash
# Terminal 1 — daemon (owns DB, grants, model routing)
cargo run -p aether-daemon

# Terminal 2 — native app
swift run AetherForgeApp
```

The app connects to `127.0.0.1:7433` via TCP JSON-lines (canonical IPC). Workspace selection saves a security-scoped bookmark under `~/Library/Application Support/AetherForge/`; each `run_task` includes `workspace_path` so the daemon can insert `capability_grants`. All agent execution goes through the daemon — the Swift app never calls git/fs tools directly.

FFI (`aether_ffi_daemon_ipc`, `aether_daemon_default_port`) provides default host/port hints only; streaming uses TCP.

## Phase 5 — Polish

- **Keychain BYOK:** `AETHER_BYOK_PROVIDER=openai` loads API key from macOS Keychain (`AetherForge` / `byok-api-key`); fail-closed on non-macOS.
- **Hybrid memory:** `search_semantic_memory_hybrid` fuses FTS5 BM25 + sqlite-vec KNN via Reciprocal Rank Fusion (MEM-01 uses hybrid path).
- **Packaging:** `./scripts/create-dmg.sh` builds a drag-and-drop DMG; `./scripts/notarize.sh` for Apple notarization when Developer ID cert is available.
- **CI:** `.github/workflows/ci.yml` — Darwin 15/15 gate, Linux 10/15 fail-closed matrix ([docs/LINUX_CI.md](docs/LINUX_CI.md)).

Install guide: [docs/INSTALL.md](docs/INSTALL.md)

## Architecture target vs product readiness

- **Spec engineering target:** 8.5+ (see v1.2.3 / v1.2.4 docs)
- **Shipped harness baseline:** 15/15 on Darwin (15 hard) when ROUT-01 warm TTFT passes, GRAPH-01 recall@3 ≥ 1.0, RED-01 0% escape, and SKILL-02 3/3 chapter routing with citation fidelity ≥ 0.9 — secure FS, sandbox, crash recovery (SIGTERM child), git grant enforcement, SSE streaming router, memory (sqlite-vec KNN + graph hop RRF), MCP stdio JSON-RPC, procedural skills, progressive-disclosure skill routing, ReAct loop, permissions, adversarial red-team suite, Python lint, daemon IPC, macOS SwiftUI shell
