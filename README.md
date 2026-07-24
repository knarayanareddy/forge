# forge

AetherForge MVP harness baseline — **Darwin 10/10** readiness (v1.2.4).

## Harness score (Darwin, canonical)

```text
PASS: FS-01, FS-02, MCP-01, MEM-01, SKILL-01, SAFE-01, RES-01, ROUT-01, GIT-01, CODE-01  →  10/10
```

Linux CI expects FS-02 and MEM-01 to fail closed when `sandbox-exec` or Ollama are unavailable.

## Layout

- Rust workspace: `Cargo.toml`, `crates/`, `tests/golden_harness/`
- Procedural skills: `skills/` (agentskills.io-style `SKILL.md`)
- Sandbox profile: `profiles/sandbox_tool.sb` (allow-default, Darwin-verified)
- MCP allowlist: `mcp_allowlist.json` (Apple Silicon `/opt/homebrew` paths)
- macOS shell (stub): `macos/AetherForgeApp/` + `Package.swift`
- Canonical specs: `docs/AetherForge_Canonical_Spec_v1.2.3.md`, `docs/AetherForge_Canonical_Spec_v1.2.4.md`

## Build & evaluate

```bash
cargo build --workspace
cargo run -p golden-harness
```

**Requirements (Darwin):** `sandbox-exec` (macOS), Ollama with `all-minilm` (MEM-01) and a chat model such as `qwen2.5:3b` (ROUT-01).

## Architecture target vs product readiness

- **Spec engineering target:** 8.5+ (see v1.2.3 / v1.2.4 docs)
- **Shipped harness baseline:** 10/10 on Darwin — secure FS, sandbox, memory (sqlite-vec KNN), MCP allowlist, procedural skills, permissions (canonical paths + subpath grants), model routing, git ops, Python lint, crash recovery, LoopEngine stub
