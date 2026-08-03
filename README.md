# forge

AetherForge MVP — **Phase 8.0 closed** (Darwin 22/22 verified at `432ace9`). Phase 9 in progress;
the harness has since grown to 23 tasks with **UNDO-01** (Phase 9 slices 9.7-9.8), which is
Linux-verified but not yet re-run through canonical Darwin CI.

## Harness score (Darwin, canonical)

```text
cargo run -p golden-harness --bin golden-harness
→ 23/23 harness target (Phase 8.0 closure verified 22/22 at 432ace9; UNDO-01 added after and is
  Linux-verified pending its first Darwin canonical run) when ROUT-01 median warm TTFT ≤ 200ms,
  GRAPH-01 recall@3 ≥ 1.0, LOOP-02 NL plan through verify shell (gold trajectory in harness eval only),
  RED-01 blocks all frozen adversarial cases (≥12, currently 14),
  SKILL-02 routes 3/3 book_skill questions with citation fidelity ≥ 0.9,
  AUTO-01 fires a granted automation trigger → run_task → audit_log,
  CHECK-01 rejects ≥8 frozen bad plans with 0 unverified writes,
  GATE-01 denies inbound without GatewayGrant then round-trips with grant,
  PLAN-01 routes ≥9/10 diverse NL goals with required tools and no forbidden tools,
  MEM-02 proves daemon turn → semantic chunk → graph link → isolated next-turn recall,
  SB-01 proves production tool sandboxing, environment scrubbing, and network denial,
  SESS-01 proves the JSONL session log records every plan/tool/observe/verify/error event
  and reconstructs the executed trajectory without re-running inference,
  and UNDO-01 proves a multi-file + git run can be undone byte-identical for every journaled
  write, with the non-undoable git_init explicitly enumerated rather than silently skipped
```

| Platform | Expected score | Hard / soft |
|----------|----------------|-------------|
| **Darwin** (Ollama + `sandbox-exec`) | **23/23 target** | 22/22 verified through `432ace9` (run `30565128737`); UNDO-01 added since, Linux-verified |
| **Linux CI** (full matrix) | **16/23** | 16 hard · FS-02, SB-01, MEM-01, ROUT-01, GRAPH-01, LOOP-02, PLAN-01 **FAIL-CLOSED** |
| **Linux Ollama-independent** | **16/16** | FS-01, SAFE-01, RES-01, GIT-01, CODE-01, MCP-01, MEM-02, SKILL-01, SKILL-02, RED-01, LOOP-01, SESS-01, **UNDO-01**, AUTO-01, CHECK-01, GATE-01 |

Tasks (23): ROUT-01, FS-01, FS-02, **SB-01**, GIT-01, CODE-01, MCP-01, MEM-01, **MEM-02**, GRAPH-01, SKILL-01, SKILL-02, SAFE-01, RED-01, RES-01, LOOP-01, LOOP-02, **PLAN-01**, **SESS-01**, **UNDO-01**, **AUTO-01**, **CHECK-01**, **GATE-01**

ROUT-01 runs first, warms the chat model, drains five discard streams, then records **seven**
server-side warm TTFT samples (`load_duration + prompt_eval_duration`). It drops the lowest and
highest sample and takes the median of the remaining five, retrying at most three rounds. The
local product target is **≤200ms**. GitHub's shared `macos-15` runner uses an explicitly looser
**550ms CI stability gate** via `AETHER_ROUT_TTFT_MS`; that infrastructure allowance is not a
product-performance claim.

**Ollama flake honesty:** GRAPH-01 and LOOP-02 may fail on cold Ollama even when ROUT-01 passes — re-run after warmup or check `Note: Ollama unreachable` lines in harness output.

## Phase status

| Phase | Scope | Status |
|-------|-------|--------|
| **1 — Foundation** | Daemon, RES recovery, GIT grants, streaming ROUT | **Done** |
| **2 — MCP** | stdio JSON-RPC client, grant-gated invoke, real filesystem MCP in harness | **Done** |
| **3 — Loop** | ReAct LoopEngine, ToolRegistry, daemon integration, LOOP-01 | **Done** |
| **4 — SwiftUI** | macOS app, TCP daemon client, workspace bookmarks | **Done** |
| **5 — Polish** | Keychain BYOK, hybrid memory RRF, DMG/notarize scripts, Linux CI | **Done** |
| **6 — Graph v1 + eval** | Bi-temporal graph, ingest extract, GRAPH-01/LOOP-02/RED-01/SKILL-02, consolidate offline | **Done** |
| **7 — Orchestration** | Automation scheduler (AUTO-01), maker-checker (CHECK-01), gateway (GATE-01) | **Done — 18/18 harness** |
| **8.0 — Honesty + closed loop** | IPC lockdown, NL de-harness, memory (MEM-02), production sandbox (SB-01) | **8.0a–8.0c implemented; docs/canonical SB-01 gate remain** |
| **9 — Trust & Time** | Planner schema/repair (PLAN-01), JSONL session log (SESS-01), undo journal (UNDO-01) | **Slices 9.1–9.8 implemented; Darwin 22/22 verified through SESS-01, UNDO-01 Linux-verified** |

See [docs/ROADMAP_PHASES_1-5.md](docs/ROADMAP_PHASES_1-5.md) for Phases 1–5 acceptance criteria.  
See [docs/ROADMAP_PHASE_6.md](docs/ROADMAP_PHASE_6.md) for Phase 6 binding spec.  
See [docs/ROADMAP_PHASE_7.md](docs/ROADMAP_PHASE_7.md) for Phase 7 orchestration + gateway contract.  
See [docs/ROADMAP_PHASE_8.0.md](docs/ROADMAP_PHASE_8.0.md) for the Phase 8.0 honesty wedge (**closed** — Darwin 22/22 verified) and [docs/PHASE_8_0_CLOSURE.md](docs/PHASE_8_0_CLOSURE.md) for the closure evidence.
See [docs/PHASE_8_0_CLOSURE.md](docs/PHASE_8_0_CLOSURE.md) for code-grounded closure evidence and remaining gates.
See [docs/ROADMAP_PHASES_9-13.md](docs/ROADMAP_PHASES_9-13.md) for the Phases 9–13 product wedge (planner robustness, session log, undo, table stakes, supply chain, local-first differentiators) plus parallel distribution and interop tracks.
See [docs/SANDBOX.md](docs/SANDBOX.md) for the production tool boundary, platform behavior, and SB-01 contract.

## Phase 7 — Orchestration + gateway (complete)

- **AutomationGrant** + scheduler registry in `aether-daemon` (`AutomationScheduler`, cron/file-watch/PR webhook stubs)
- Triggers require explicit grant per `trigger_id`; denied runs audit to `audit_log`
- **AUTO-01:** frozen cron fixture → enqueue → `run_task` LOOP-01 mini-plan → `audit_log` with `trigger_id`
- Optional Darwin hooks: `scripts/install-automation-hooks.sh` (launchd/cron stub)
- **CHECK-01:** `OrchestrationGraph` + read-only `VerifierNode` deny unverified `fs_write` (0/8 bad plans escape)
- **GATE-01:** `GatewayGrant` + `GatewayRouter` with Slack-first adapter, Telegram/Discord stubs, localhost mock — [docs/GATEWAY.md](docs/GATEWAY.md)

## Layout

- Rust workspace: `Cargo.toml`, `crates/`, `tests/golden_harness/`
- **Daemon:** `crates/aether-daemon/` — TCP JSON-lines on `127.0.0.1:7433`
- **Loop engine:** `crates/aether-core/src/loop_engine.rs` — plan → tool → observe → verify
- **Graph v1:** `crates/aether-db/src/graph.rs` — bi-temporal wiki zone; see [docs/GRAPH_V1.md](docs/GRAPH_V1.md)
- Procedural skills: `skills/` (agentskills.io-style `SKILL.md`); progressive disclosure in [docs/RATEL_TOOL_INDEX.md](docs/RATEL_TOOL_INDEX.md)
- Sandbox profile: `profiles/sandbox_tool.sb` (allow-default, Darwin-verified)
- MCP allowlist: `mcp_allowlist.json` (SHA-256 pins for node + server script + `tools_hash_pin`; runtime resolves paths via `AETHER_MCP_NODE` / `AETHER_MCP_FILESYSTEM_SCRIPT` or `npm root -g`)
- macOS app: `macos/AetherForgeApp/` + `Package.swift` + `macos/AetherFFI/` (C ABI hints)
- Specs: `docs/AetherForge_Canonical_Spec_v1.2.4.md`, `docs/ROADMAP_PHASES_1-5.md`, `docs/ROADMAP_PHASE_6.md`, `docs/ROADMAP_PHASE_7.md`, `docs/GRAPH_V1.md`, `docs/RATEL_TOOL_INDEX.md`, `docs/DAEMON_IPC.md`, `docs/INSTALL.md`, `docs/LINUX_CI.md`

## Build & evaluate

```bash
cargo build --workspace
cargo run -p golden-harness --bin golden-harness
```

**Requirements (Darwin):** `sandbox-exec` (macOS), Ollama with `all-minilm` (MEM-01) and a chat model such as `qwen2.5:3b` (ROUT-01, GRAPH-01, LOOP-02), Node.js with `@modelcontextprotocol/server-filesystem` (MCP-01).

## Agent loop (Phase 3 + 6)

`ReActLoopEngine` runs structured plans: **plan → tool → observe → verify**, capped by `max_iterations` and token budget (`LoopConfig` telemetry — Slice 6.3).

**ToolRegistry** wires grant-checked: FS read/write, git (`GitOps`), MCP (`invoke_with_grant`), skills (`SkillExecutor`), Python lint (`PythonLinter`).

Plain-text prompts route through `NlPlanner` (LOOP-02) when no `{"loop":...}` JSON is present — same verify shell as LOOP-01.

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

## MCP client (Phase 2)

`crates/aether-mcp/src/runtime.rs` provides a minimal stdio JSON-RPC MCP client:

- Load allowlist → SHA-256 verify node + entry script → spawn
- `initialize` → `tools/list` (hashes tool descriptions; verifies `tools_hash_pin` when set)
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

The app connects to `127.0.0.1:7433` via TCP JSON-lines (canonical IPC). Workspace selection saves
a security-scoped bookmark under `~/Library/Application Support/AetherForge/`; after that explicit
user action, the app sends authenticated `grant_workspace` before `run_task`. Execution paths
require the pre-existing grant and never grant themselves authority. All agent execution goes
through the daemon — the Swift app never calls git/fs tools directly.

FFI (`aether_ffi_daemon_ipc`, `aether_daemon_default_port`) provides default host/port hints only; streaming uses TCP.

## Phase 5 — Polish

- **Keychain BYOK:** `AETHER_BYOK_PROVIDER=openai` loads API key from macOS Keychain (`AetherForge` / `byok-api-key`); fail-closed on non-macOS.
- **Hybrid memory:** `search_semantic_memory_hybrid` fuses FTS5 BM25 + sqlite-vec KNN via Reciprocal Rank Fusion (MEM-01 uses hybrid path).
- **Packaging:** `./scripts/create-dmg.sh` builds a drag-and-drop DMG; `./scripts/notarize.sh` for Apple notarization when Developer ID cert is available.

## Phase 6 — Graph v1 memory wedge + eval hardening

- **Graph v1:** bi-temporal `graph_nodes` / `graph_edges`, 1-hop RRF in `search_hybrid_with_graph` — [docs/GRAPH_V1.md](docs/GRAPH_V1.md)
- **Ingest extract:** Ollama `graph_extract` on session post-turn hook (failures audit-logged)
- **NL planner:** LOOP-02 through same verify shell as LOOP-01
- **RED-01:** ≥12 frozen adversarial cases (14 shipped); 0% forbidden-action escape
- **SKILL-02:** book-to-skill progressive disclosure — [docs/RATEL_TOOL_INDEX.md](docs/RATEL_TOOL_INDEX.md)
- **Consolidate offline:** `./scripts/consolidate_memory.sh` → `review_pending` until human apply
- **CI:** `.github/workflows/ci.yml` — Linux full matrix gate **≥16/23** · Darwin PR **build + unit tests + Swift only** · push/nightly Darwin **23/23 target** ([docs/LINUX_CI.md](docs/LINUX_CI.md))

Install guide: [docs/INSTALL.md](docs/INSTALL.md)

## Architecture target vs product readiness

- **Spec engineering target:** 8.5+ (see v1.2.3 / v1.2.4 docs; Phase 6 memory architecture via [ROADMAP_PHASE_6.md](docs/ROADMAP_PHASE_6.md))
- **Last verified main baseline:** **22/22 on Darwin** (run `30565128737`) — Phase 7 capabilities plus schema-constrained planner repair (PLAN-01), closed daemon semantic-memory recall (MEM-02), authenticated IPC lockdown, production tool sandboxing (SB-01), and the JSONL session log (SESS-01).
