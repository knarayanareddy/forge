# Linux CI Expectations

AetherForge treats **Darwin (macOS 15+)** as the canonical platform. Linux CI validates cross-platform Rust crates and documents explicit fail-closed behavior for platform-specific harness tasks.

## Harness matrix

| Task | Tier | Linux CI | Reason |
|------|------|----------|--------|
| FS-01 | hard | PASS | Grant-checked `FileMutator` — OS-agnostic |
| FS-02 | hard | **FAIL-CLOSED** | Requires macOS `sandbox-exec` + Seatbelt profile |
| GIT-01 | hard | PASS | Real git subprocess with grant gate |
| CODE-01 | hard | PASS | `python3 -m py_compile` |
| MCP-01 | hard | PASS* | Requires Node + MCP server installed in CI |
| ROUT-01 | hard | **FAIL-CLOSED** | Requires live Ollama SSE streaming |
| MEM-01 | hard | **FAIL-CLOSED** | Requires Ollama `all-minilm` embeddings |
| SKILL-01 | hard | PASS | Procedural skill loader |
| SAFE-01 | hard | PASS | Permission + audit hash chain |
| RES-01 | hard | PASS | SIGTERM child recovery (uses Unix signals) |
| LOOP-01 | hard | PASS | ReAct loop in production crate |

\* MCP-01 fails if `@modelcontextprotocol/server-filesystem` is not installed — install via `npm install -g` in CI.

## Expected scores

| Environment | Expected harness | Notes |
|-------------|------------------|-------|
| Darwin + Ollama + sandbox-exec | **11/11 (11 hard)** | Canonical release gate |
| Linux (default CI) | **8/11** | FS-02, MEM-01, ROUT-01 fail-closed |
| Linux + Ollama service | **10/11** | FS-02 still fail-closed without sandbox-exec |

## CI workflow

GitHub Actions (`.github/workflows/ci.yml`) runs:

1. `cargo build --workspace`
2. `cargo test --workspace`
3. `cargo run -p golden-harness` (results interpreted per matrix above)

Linux jobs **must not** skip fail-closed tasks silently — harness prints `FAIL-CLOSED` for FS-02, MEM-01, ROUT-01 when prerequisites are absent.

## BYOK on Linux

Setting `AETHER_BYOK_PROVIDER` on non-macOS causes daemon startup to **fail closed** (Keychain unavailable). Do not set BYOK env vars in Linux CI.

## Local reproduction

```bash
# Full Darwin gate
cargo run -p golden-harness

# Simulate Linux fail-closed (unset Ollama, non-Darwin only)
# On macOS, FS-02 still passes if sandbox-exec exists.
```
