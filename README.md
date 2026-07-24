# forge

AetherForge MVP harness baseline — **Darwin 7/10** readiness (v1.2.4).

## Harness score (Darwin, canonical)

```text
PASS: FS-01, FS-02, MCP-01, MEM-01, SKILL-01, SAFE-01, RES-01  →  7/10
RED:  GIT-01, CODE-01, ROUT-01
```

Linux CI expects FS-02 and MEM-01 to fail closed when `sandbox-exec` or Ollama are unavailable.

## Layout

- Rust workspace: `Cargo.toml`, `crates/`, `tests/golden_harness/`
- Procedural skills: `skills/` (agentskills.io-style `SKILL.md`)
- Sandbox profile: `profiles/sandbox_tool.sb` (allow-default, Darwin-verified)
- MCP allowlist: `mcp_allowlist.json` (Apple Silicon `/opt/homebrew` paths)
- Canonical specs: `docs/AetherForge_Canonical_Spec_v1.2.3.md`, `docs/AetherForge_Canonical_Spec_v1.2.4.md`

## Build & evaluate

```bash
cargo build --workspace
cargo run -p golden-harness
```

**Requirements (Darwin):** `sandbox-exec` (macOS), Ollama with `all-minilm` model for MEM-01.

## Architecture target vs product readiness

- **Spec engineering target:** 8.5+ (see v1.2.3 / v1.2.4 docs)
- **Shipped harness baseline:** 7/10 on Darwin — secure FS, sandbox, memory, MCP allowlist, procedural skills, permissions, crash recovery
