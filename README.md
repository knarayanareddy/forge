# forge

AetherForge MVP — **Phase 1 Foundation** complete on Darwin.

## Harness score (Darwin, canonical)

```text
cargo run -p golden-harness
→ 10/10 harness (10 hard / 0 soft)
```

Tasks: FS-01, FS-02, GIT-01, CODE-01, MCP-01, MEM-01, SKILL-01, SAFE-01, ROUT-01, RES-01

Linux CI expects FS-02, MEM-01, and ROUT-01 to fail closed when `sandbox-exec` or Ollama are unavailable.

## Phase status

| Phase | Scope | Status |
|-------|-------|--------|
| **1 — Foundation** | Daemon, RES recovery, GIT grants, streaming ROUT | **Done** |
| 2 — MCP | Daemon-routed MCP invoke | Planned |
| 3 — Loop | Real LoopEngine | Planned |
| 4 — SwiftUI | macOS app + TCP client | Planned |
| 5 — Polish | DMG, notarization, CI matrix | Planned |

See [docs/ROADMAP_PHASES_1-5.md](docs/ROADMAP_PHASES_1-5.md) for acceptance criteria and audit gates.

## Layout

- Rust workspace: `Cargo.toml`, `crates/`, `tests/golden_harness/`
- **Daemon:** `crates/aether-daemon/` — TCP JSON-lines on `127.0.0.1:7433`
- Procedural skills: `skills/` (agentskills.io-style `SKILL.md`)
- Sandbox profile: `profiles/sandbox_tool.sb` (allow-default, Darwin-verified)
- MCP allowlist: `mcp_allowlist.json` (Apple Silicon `/opt/homebrew` paths)
- macOS shell (stub): `macos/AetherForgeApp/` + `Package.swift`
- Specs: `docs/AetherForge_Canonical_Spec_v1.2.4.md`, `docs/ROADMAP_PHASES_1-5.md`, `docs/DAEMON_IPC.md`

## Build & evaluate

```bash
cargo build --workspace
cargo run -p golden-harness
```

**Requirements (Darwin):** `sandbox-exec` (macOS), Ollama with `all-minilm` (MEM-01) and a chat model such as `qwen2.5:3b` (ROUT-01).

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
- **Shipped harness baseline:** 10/10 on Darwin (10 hard) — secure FS, sandbox, crash recovery (SIGTERM child), git grant enforcement, SSE streaming router, memory (sqlite-vec KNN), MCP allowlist, procedural skills, permissions, Python lint, daemon IPC
